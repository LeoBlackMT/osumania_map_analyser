#!/usr/bin/env python3
"""行为数据集评测工具（「终极裁判」的可用部分）。

用途：对**已有路由**的任何改动做改动前 / 改动后的系统偏差比对。数据集记录的是
`delta = play_implied_dan − 插件输出`，所以每个 (侧, 路由, 层级带, 倍速, lnRatio) 分组上的
delta 均值就是该路由的**系统偏差**；改动后重跑本工具、比较 bias 是否向 0 收敛即可。

数据来源（外部，不入库）：Mania Tracker / mania-hub 导出的 charts_4k.csv / charts_7k.csv。
来源链：上游估算 → 锚定难度 → 484 万次实战成绩（dan-credit 的 ACC 积分曲线反推段位）→ 残差拟合。

用法：
    python behaviour-eval.py                      # 打印报告并写快照
    python behaviour-eval.py --out before.json    # 指定快照路径
    python behaviour-eval.py --compare before.json after.json   # 比对两次运行

判据（硬要求，见 docs/learnings/difficulty-estimation.md §5.1/§5.2）：
    * 用**带符号 bias** 与**改善行占比**判断，不要用 MAE —— delta 的组内 sd（≈0.9~1.1）
      远大于待检偏差（0.1~0.6），MAE 看不见均值漂移，同规模安慰剂常胜过真特征。
    * 拟合参数只能在训练折上拟合；本工具的"分组同号性"自检只能确认偏差的分组稳定性，
      不能替代真正的 holdout。
    * 分组前先核对量纲：被比较的量必须就是被修正的量（RC 半段 vs LN 半段不可混）。
"""

import argparse
import csv
import json
import math
import os
import sys
from collections import defaultdict

DEFAULT_BASE = r"C:\Users\Leo_BlackLT\Desktop\Dev\files\mania-tracker-dataset\out"
LEVEL_BANDS = [(-99, 2), (2, 4), (4, 6), (6, 8), (8, 10), (10, 12), (12, 14), (14, 99)]
RATE_BINS = [0.75, 1.0, 1.5]


def num(v):
    if v is None:
        return None
    v = v.strip()
    if v == "":
        return None
    try:
        return float(v)
    except ValueError:
        return None


def load(path, min_players):
    rows = []
    with open(path, newline="", encoding="utf-8") as fh:
        for r in csv.DictReader(fh):
            if r.get("in_table") != "True":
                continue
            if (num(r.get("n_players")) or 0) < min_players:
                continue
            d = num(r.get("delta"))
            lv = num(r.get("leo_numeric"))
            if d is None or lv is None:
                continue
            rows.append(dict(
                algo=r.get("algo") or "?",
                side=r.get("side") or "?",
                delta=d,
                level=lv,
                rate=num(r.get("rate")),
                ln_ratio=num(r.get("ln_ratio")),
                bid=r.get("beatmap_id") or "",
            ))
    return rows


def band_of(level):
    for i, (lo, hi) in enumerate(LEVEL_BANDS):
        if lo < level <= hi:
            return i
    return len(LEVEL_BANDS) - 1


def summarize(values):
    n = len(values)
    if not n:
        return None
    m = sum(values) / n
    sd = math.sqrt(sum((x - m) ** 2 for x in values) / n) if n > 1 else 0.0
    se = sd / math.sqrt(n) if n else 0.0
    return {
        "n": n,
        "bias": round(m, 4),
        "sd": round(sd, 4),
        "mae": round(sum(abs(x) for x in values) / n, 4),
        "se": round(se, 4),
        "bias_over_se": round(abs(m) / se, 2) if se else None,
    }


def build_snapshot(rows, label):
    snap = {"label": label, "overall": summarize([r["delta"] for r in rows]), "groups": {}}
    groups = defaultdict(list)
    for r in rows:
        groups[("side", r["side"])].append(r["delta"])
        groups[("route", r["side"], r["algo"])].append(r["delta"])
        groups[("band", r["side"], r["algo"], band_of(r["level"]))].append(r["delta"])
        if r["rate"] in RATE_BINS:
            groups[("rate", r["side"], r["rate"])].append(r["delta"])
    for key, values in groups.items():
        s = summarize(values)
        if s and s["n"] >= 15:
            snap["groups"]["|".join(str(k) for k in key)] = s
    # 分组稳定性：按 beatmap_id % 10 分折的逐折均值（同号比例越高越说明是真实尺度差）
    folds = defaultdict(list)
    for r in rows:
        folds[int(r["bid"] or 0) % 10].append(r["delta"])
    means = [round(sum(v) / len(v), 4) for _, v in sorted(folds.items()) if v]
    snap["fold_means"] = means
    snap["fold_positive"] = sum(1 for m in means if m > 0)
    snap["fold_total"] = len(means)
    return snap


def print_report(snap):
    print(f"\n{'=' * 88}\n{snap['label']}")
    o = snap["overall"]
    print(f"  总体  n={o['n']}  bias={o['bias']:+.4f}  sd={o['sd']:.3f}  MAE={o['mae']:.4f}  "
          f"|bias|/se={o['bias_over_se']}")
    for kind, title in (("side", "按侧"), ("route", "按路由"), ("band", "按侧×路由×层级带"), ("rate", "按侧×倍速")):
        keys = [k for k in snap["groups"] if k.startswith(kind + "|")]
        if not keys:
            continue
        print(f"\n  -- {title} --")
        for k in keys:
            g = snap["groups"][k]
            print(f"    {k[len(kind) + 1:]:<24} n={g['n']:>6}  bias={g['bias']:+.4f}  "
                  f"MAE={g['mae']:.4f}  |bias|/se={g['bias_over_se']}")
    print(f"\n  分组稳定性（beatmap_id % 10）：{snap['fold_positive']}/{snap['fold_total']} 折为正  "
          f"逐折 " + " ".join(f"{m:+.2f}" for m in snap["fold_means"]))


def print_compare(a, b):
    print(f"\n{'=' * 88}\n改动前 → 改动后")
    print(f"  A = {a['label']}    B = {b['label']}")
    print(f"\n  {'分组':<26} {'n(A→B)':>14} {'bias A':>9} {'bias B':>9} {'Δbias':>9} {'|bias| 收敛':>12}")
    keys = sorted(set(a["groups"]) | set(b["groups"]))
    moved, improved, worsened = [], 0, 0
    for k in keys:
        ga, gb = a["groups"].get(k), b["groups"].get(k)
        if not ga or not gb:
            continue
        d = gb["bias"] - ga["bias"]
        better = abs(gb["bias"]) < abs(ga["bias"]) - 1e-4
        worse = abs(gb["bias"]) > abs(ga["bias"]) + 1e-4
        improved += 1 if better else 0
        worsened += 1 if worse else 0
        tag = "改善" if better else ("变差" if worse else "持平")
        print(f"  {k:<26} {ga['n']:>6}→{gb['n']:<6} {ga['bias']:>+9.4f} {gb['bias']:>+9.4f} "
              f"{d:>+9.4f} {tag:>12}")
        if abs(d) > 1e-4:
            moved.append(k)
    print(f"\n  分组收敛：改善 {improved} / 变差 {worsened} / 总计 {len(keys)}")
    print(f"  （判据是 |bias| 收敛与分组同号性；**不要**改用 MAE —— 见文件头）")
    if not moved:
        print("  ⚠️ 所有分组 bias 未变——若本次改动本应影响这些路由，请先检查改动是否真的生效。")


def main():
    ap = argparse.ArgumentParser(description="行为数据集评测（改动前/后系统偏差比对）")
    ap.add_argument("--base", default=DEFAULT_BASE, help="数据集目录（含 charts_4k.csv / charts_7k.csv）")
    ap.add_argument("--keys", default="4k,7k", help="逗号分隔：4k,7k")
    ap.add_argument("--min-players", type=int, default=8)
    ap.add_argument("--out", default="", help="把本次快照写到该 JSON 路径")
    ap.add_argument("--label", default="", help="快照标签（如 before / after）")
    ap.add_argument("--compare", nargs=2, metavar=("A.json", "B.json"),
                    help="比对两份快照，不读取数据集")
    args = ap.parse_args()

    if args.compare:
        a = json.load(open(args.compare[0], encoding="utf-8"))
        b = json.load(open(args.compare[1], encoding="utf-8"))
        print_compare(a, b)
        return

    snapshots = []
    for km in args.keys.split(","):
        km = km.strip()
        path = os.path.join(args.base, f"charts_{km}.csv")
        if not os.path.exists(path):
            print(f"[skip] {path} 不存在")
            continue
        rows = load(path, args.min_players)
        sn = build_snapshot(rows, args.label or f"{km.upper()} ({len(rows)} rows)")
        print_report(sn)
        snapshots.append(sn)

    if args.out:
        payload = snapshots[0] if len(snapshots) == 1 else snapshots
        with open(args.out, "w", encoding="utf-8") as fh:
            json.dump(payload, fh, ensure_ascii=False, indent=1)
        print(f"\n快照已写入 {args.out}（改动后再跑一次，用 --compare 比对）")


if __name__ == "__main__":
    sys.exit(main())

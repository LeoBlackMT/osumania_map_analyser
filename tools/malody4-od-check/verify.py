#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Malody 4.3.7（PC 端）判定档 × 速率 → 等效 osu!mania OD：表重算 + 代码表逐值校验。

数据来源（PC 端窗口数值只出现在本文件里，JS 侧不复制）：
  `Malody-4.3.7判定-OD等效表.md`（方法学）与 `判定窗口-4.3.7-桌面版.md`
  （malody.exe 4.3.7 反汇编：Key 默认组 C 档 36/76/110，A~E 偏移 +20/+10/0/−8/−16，
  自动 MISS 桌面端固定 160 ms，速率把窗口按 ×1/rate 缩放）。
方法：96% 准确率等精度（σ*）等价——Malody 侧解出「误差服从零均值正态且期望准确率恰为
  96%」的 σ*，再解 osu 侧达到同一 σ* 的 OD（搜索区间 [−5, 21.3]）。
纪律：误差分布或准确率口径一旦改动，整张表必须重算；**不得为对齐某个样本手改数值**。

运行：python tools/malody4-od-check/verify.py [--emit-md]

为什么放在 tools/ 而不是 tests/：CLAUDE.md 规定测试脚本一律不得提交进仓库，且
  .gitignore 含 `/tests`；tools/ 是本仓库既有的开发工装目录，与「可复算的数值表校验器」
  定位一致。测试脚本（tests/ 下）不入库。
"""

import argparse
import math
import os
import re
import sys

# ── 输入 1：Malody 4.3.7 PC 端 Key 判定窗口（ms）──
JUDGE_BASE_MS = (36.0, 76.0, 110.0)                 # C 档：BEST / COOL / GOOD（外侧）
JUDGE_LEVEL_OFFSET_MS = {"A": 20.0, "B": 10.0, "C": 0.0, "D": -8.0, "E": -16.0}
AUTO_MISS_MS = 160.0                                # 桌面端固定，与判定档无关
MALODY_ACC_WEIGHTS = (1.0, 0.75, 0.25, 0.0)         # BEST / COOL / GOOD / MISS
JUDGE_LEVELS = ("A", "B", "C", "D", "E")
RATES = (("NM", 1.0), ("DASH(1.2)", 1.2), ("RUSH(1.5)", 1.5), ("SLOW(0.8)", 0.8))

# ── 输入 2：osu!mania 判定窗口 w(OD) = DifficultyRange(od, v0, v5, v10) ──
# 值取自 osu!mania 判定窗口的 DifficultyRange 三元组（OD 0 / 5 / 10）；本口径把 w 当作
# ± 命中界限（与产出 Step 13 表值的既有工装一致），换口径等于换一整张表。
OSU_JUDGE_RANGES = (
    (22.4, 19.4, 13.9),      # PERFECT
    (64.0, 49.0, 34.0),      # GREAT (300)
    (97.0, 82.0, 67.0),      # GOOD (200)
    (127.0, 112.0, 97.0),    # OK (100)
    (151.0, 136.0, 121.0),   # MEH (50)
    (188.0, 173.0, 158.0),   # MISS
)
# ScoreV1 口径、无 mod、最高档归一为 1.0；PERFECT 与 GREAT 同为 1.0 ⇒ 相减后抵消
OSU_ACC_WEIGHTS = (1.0, 1.0, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 6.0, 0.0)

TARGET_ACCURACY = 0.96
OD_LO, OD_HI = -5.0, 21.3       # 上界 21.3：Sunny 的 sqrt 在 od > 21.3̄ 时无定义域保护
OD_MATCH_TOL = 0.005            # 与 JS 表（两位小数）的允许差
RESIDUAL_MAX_MS = 0.05          # 「精确命中 σ*」而不是在搜索边界饱和

JS_TABLE_REL = os.path.join("ManiaMapAnalyser by Leo_Black", "js", "parser", "judgeOdTable.js")
JUDGE_ROW_RE = re.compile(
    r"^\s*(?P<judge>[A-E])\s*:\s*Object\.freeze\(\{\s*"
    r"NM\s*:\s*(?P<NM>-?\d+(?:\.\d+)?)\s*,\s*"
    r"DASH\s*:\s*(?P<DASH>-?\d+(?:\.\d+)?)\s*,\s*"
    r"RUSH\s*:\s*(?P<RUSH>-?\d+(?:\.\d+)?)\s*,\s*"
    r"SLOW\s*:\s*(?P<SLOW>-?\d+(?:\.\d+)?)\s*\}\)",
    re.MULTILINE,
)
RATE_FIELD = {"NM": "NM", "DASH(1.2)": "DASH", "RUSH(1.5)": "RUSH", "SLOW(0.8)": "SLOW"}


def phi(z):
    """标准正态 CDF。"""
    return 0.5 * (1.0 + math.erf(z / math.sqrt(2.0)))


def expected_accuracy(sigma, windows, weights):
    """误差 ~ N(0, sigma) 时的期望准确率：按档累加「该档权重 × 落入该档的概率」。

    windows 升序、weights 为各档得分比；最外一档权重为 0 时该项恒被消掉。
    """
    total = 0.0
    prev = 0.0
    for window, weight in zip(windows, weights):
        cdf = 2.0 * phi(window / sigma) - 1.0
        total += weight * (cdf - prev)
        prev = cdf
    return total


def solve_sigma(windows, weights):
    """解 acc(sigma) = 96% 的 sigma*（acc 随 sigma 单调递减）。"""
    lo, hi = 1e-3, 500.0
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        if expected_accuracy(mid, windows, weights) > TARGET_ACCURACY:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def osu_window(od, v0, v5, v10):
    """osu 侧 DifficultyRange：od ≤ 5 用 v0→v5，od ≥ 5 用 v5→v10。"""
    if od > 5.0:
        return v5 + (v10 - v5) * (od - 5.0) / 5.0
    if od < 5.0:
        return v0 + (v5 - v0) * od / 5.0
    return v5


def osu_sigma(od):
    """osu 侧 96% 等精度解出的 sigma（随 OD 单调递减）。"""
    windows = [osu_window(od, *r) for r in OSU_JUDGE_RANGES]
    return solve_sigma(windows, OSU_ACC_WEIGHTS)


def solve_od(sigma_target):
    """解 sigma_osu(od) = sigma*，返回 (od, 残差 ms)。

    区间内无解（sigma* 超出 osu 侧可达范围）时取最接近的边界，不夹断到 0~10。
    """
    gap_lo = osu_sigma(OD_LO) - sigma_target
    gap_hi = osu_sigma(OD_HI) - sigma_target
    if gap_lo < 0.0:
        return OD_LO, abs(gap_lo)
    if gap_hi > 0.0:
        return OD_HI, abs(gap_hi)
    lo, hi = OD_LO, OD_HI
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        if osu_sigma(mid) - sigma_target > 0.0:
            lo = mid
        else:
            hi = mid
    od = 0.5 * (lo + hi)
    return od, abs(osu_sigma(od) - sigma_target)


def malody_windows(judge, rate, outer_band):
    """Malody 侧四档窗口（ms，真实时间）：三档基础窗口 + 最外档，再按 1/rate 缩放。"""
    offset = JUDGE_LEVEL_OFFSET_MS[judge]
    windows = [(base + offset) / rate for base in JUDGE_BASE_MS]
    windows.append(outer_band / rate)
    return windows


def outer_band_ms(judge, variant):
    """最外档（MISS/自动 MISS）的三种取法（其权重为 0，本应不影响结果）。"""
    if variant == "160":
        return AUTO_MISS_MS
    if variant == "150":
        return 150.0
    return min(AUTO_MISS_MS, JUDGE_BASE_MS[2] + JUDGE_LEVEL_OFFSET_MS[judge])


def solve_table(variant="160"):
    """解出 20 格：(judge, rateName) -> {sigma, od, residual}。"""
    table = {}
    for judge in JUDGE_LEVELS:
        for rate_name, rate in RATES:
            sigma = solve_sigma(malody_windows(judge, rate, outer_band_ms(judge, variant)),
                                MALODY_ACC_WEIGHTS)
            od, residual = solve_od(sigma)
            table[(judge, rate_name)] = {"sigma": sigma, "od": od, "residual": residual}
    return table


def read_js_table(repo_root):
    """用正则从 judgeOdTable.js 提取 20 格（不需要 Node）。"""
    path = os.path.join(repo_root, JS_TABLE_REL)
    if not os.path.isfile(path):
        raise RuntimeError("JS table not found: %s" % path)
    with open(path, "r", encoding="utf-8") as handle:
        source = handle.read()
    rows = {}
    for match in JUDGE_ROW_RE.finditer(source):
        rows[match.group("judge")] = {field: float(match.group(field))
                                      for field in ("NM", "DASH", "RUSH", "SLOW")}
    if sorted(rows) != list(JUDGE_LEVELS):
        raise RuntimeError("cannot extract rows A-E from %s (regex missed: format changed?)"
                           % JS_TABLE_REL)
    return rows


def format_md(table):
    """与文档页同形的 markdown 表（含每格 σ*）；表头用 ASCII 以免控制台乱码。"""
    lines = ["| Judge | " + " | ".join(name for name, _r in RATES) + " |",
             "|---|---|---|---|---|"]
    for judge in JUDGE_LEVELS:
        cells = ["%.2f" % table[(judge, name)]["od"] for name, _r in RATES]
        lines.append("| **%s** | %s |" % (judge, " | ".join(cells)))
    lines.append("")
    lines.append("sigma* per cell (ms; smaller = stricter judge windows):")
    lines.append("")
    lines.append("| Judge | " + " | ".join(name for name, _r in RATES) + " |")
    lines.append("|---|---|---|---|---|")
    for judge in JUDGE_LEVELS:
        cells = ["%.2f" % table[(judge, name)]["sigma"] for name, _r in RATES]
        lines.append("| **%s** | %s |" % (judge, " | ".join(cells)))
    return "\n".join(lines)


def main(argv):
    parser = argparse.ArgumentParser(
        description="Recompute the Malody 4.3.7 (PC) judge x rate -> equivalent osu!mania OD "
                    "table from the judge windows and compare every cell with the table in "
                    "js/parser/judgeOdTable.js.")
    parser.add_argument("--emit-md", action="store_true",
                        help="print the markdown tables (same shape as the docs page, plus "
                             "per-cell sigma*) ahead of the verification summary")
    args = parser.parse_args(argv)

    repo_root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    table = solve_table("160")
    if args.emit_md:
        print(format_md(table))
        print("")

    try:
        js_rows = read_js_table(repo_root)
    except RuntimeError as error:
        print("ERROR: %s" % error)
        return 1

    failures = []
    max_residual = 0.0
    print("%-5s %-9s %12s %12s %12s %10s %12s"
          % ("judge", "rate", "sigma*(ms)", "OD(solved)", "OD(js)", "|diff|", "residual(ms)"))
    for judge in JUDGE_LEVELS:
        for rate_name, _rate in RATES:
            cell = table[(judge, rate_name)]
            js_od = js_rows[judge][RATE_FIELD[rate_name]]
            diff = abs(cell["od"] - js_od)
            max_residual = max(max_residual, cell["residual"])
            print("%-5s %-9s %12.6f %12.6f %12.2f %10.6f %12.2e"
                  % (judge, rate_name, cell["sigma"], cell["od"], js_od, diff, cell["residual"]))
            if diff > OD_MATCH_TOL:
                failures.append("%s+%s: recomputed %.6f vs JS table %.2f (diff %.6f > %.3f)"
                                % (judge, rate_name, cell["od"], js_od, diff, OD_MATCH_TOL))
            if cell["residual"] > RESIDUAL_MAX_MS:
                failures.append("%s+%s: sigma residual %.3f ms > %.2f ms (OD saturated at the "
                                "search boundary instead of reproducing sigma*)"
                                % (judge, rate_name, cell["residual"], RESIDUAL_MAX_MS))

    # 稳健性（§2 #24b）：最外档权重为 0，三种取法必须给出同一张表。
    variants = {name: solve_table(name) for name in ("150", "min(160,w3+off)")}
    variant_diff = 0.0
    for name, other in variants.items():
        diff = max(abs(table[key]["od"] - other[key]["od"]) for key in table)
        variant_diff = max(variant_diff, diff)
        if diff > 1e-9:
            failures.append("outer band %s changed the table (max diff %.6f OD)" % (name, diff))
    print("outer band: 160 / 150 / min(160, w3+off) = %s vs %s vs %s ms"
          % ("%.0f" % AUTO_MISS_MS, "150",
             "/".join("%.0f" % outer_band_ms(j, "min") for j in JUDGE_LEVELS)))
    print("max residual = %.3e ms, max outer-band diff = %.6f OD" % (max_residual, variant_diff))

    if failures:
        print("MISMATCH:")
        for line in failures:
            print("  - %s" % line)
        return 1
    print("20/20 matched, max residual <= 0.05 ms, outer-band variants agree")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

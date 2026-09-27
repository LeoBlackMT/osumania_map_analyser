#!/usr/bin/env python3
"""Malody V (6.7.x) judge window -> equivalent osu!mania OD.

Same methodology as tools/malody4-od-check/verify.py (96%-accuracy equal-precision
sigma*), reusing that tool's solver shape verbatim; only the inputs differ because
Malody V is a different client with its own — wider — window set.

Inputs
  * window values: the two groups measured for Malody V (normal / strict-under-Pro),
    four bands Best/Cool/Good/Miss, plus the per-level offsets.
    Source: js/app/sources/odResolver.js (which itself records the APK material
    6.7.22 build 382 plus the 2026-09-24 live read-only memory measurement).
  * window scaling: Malody V's judge threshold = window value x playSpeed, i.e. the
    window in real time divides by the rate ("compensated"). Only applied when the
    play info carries the exact-speed record — with no such record the windows are
    untouched, which is the case for Mod-derived rates (Dash/Rush/Slow) and for the
    Turbo branch as measured here.
  * osu!mania side: DifficultyRange triplets at OD 0/5/10, ScoreV1 weights, 96% target.

Usage
  python tools/malody-v-od-check/verify.py            # recompute + self-checks
  python tools/malody-v-od-check/verify.py --emit-md  # print an MD table
  python tools/malody-v-od-check/verify.py --emit-js  # print the JS table body
"""

import argparse
import math
import os
import re
import sys

# ── input 1: Malody V Key-mode judge windows (ms, Best/Cool/Good/Miss) ──
# C level is the measured base of each group; the other levels are that base plus
# the per-level offset. The Miss band is unused by the accuracy formula (weight 0).
NORMAL_BASE = (45.0, 85.0, 130.0, 170.0)   # Pro OFF  (measured live: [45,85,130,170])
STRICT_BASE = (36.0, 76.0, 110.0, 160.0)   # Pro ON   (user-authoritative: strict group)
LEVEL_OFFSET_MS = {0: 20.0, 1: 10.0, 2: 0.0, 3: -8.0, 4: -16.0}
MALODY_ACC_WEIGHTS = (1.0, 0.75, 0.25, 0.0)   # Best / Cool / Good / Miss

# ── input 2: osu!mania windows (identical to the 4.3.7 tool, same methodology) ──
OSU_JUDGE_RANGES = (
    (22.4, 19.4, 13.9),
    (64.0, 49.0, 34.0),
    (97.0, 82.0, 67.0),
    (127.0, 112.0, 97.0),
    (151.0, 136.0, 121.0),
    (188.0, 173.0, 158.0),
)
OSU_ACC_WEIGHTS = (1.0, 1.0, 2.0 / 3.0, 1.0 / 3.0, 1.0 / 6.0, 0.0)

TARGET_ACCURACY = 0.96
OD_LO, OD_HI = -5.0, 21.3
RESIDUAL_MAX_MS = 0.05
OD_MATCH_TOL = 0.005    # stored JS table is written to 2 decimals, so compare within this

# nominal rates of the three rate-changing mods, and their shell window scales
MOD_RATES = (("NM", 1.0), ("DASH", 1.2), ("RUSH", 1.5), ("SLOW", 0.8))


def phi(z):
    return 0.5 * (1.0 + math.erf(z / math.sqrt(2.0)))


def expected_accuracy(sigma, windows, weights):
    total = 0.0
    prev = 0.0
    for window, weight in zip(windows, weights):
        cdf = 2.0 * phi(window / sigma) - 1.0
        total += weight * (cdf - prev)
        prev = cdf
    return total


def solve_sigma(windows, weights):
    lo, hi = 1e-3, 500.0
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        if expected_accuracy(mid, windows, weights) > TARGET_ACCURACY:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def osu_window(od, v0, v5, v10):
    if od > 5.0:
        return v5 + (v10 - v5) * (od - 5.0) / 5.0
    if od < 5.0:
        return v0 + (v5 - v0) * od / 5.0
    return v5


def osu_sigma(od):
    return solve_sigma([osu_window(od, *r) for r in OSU_JUDGE_RANGES], OSU_ACC_WEIGHTS)


def solve_od(sigma_target):
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


def base_windows(judge, pro):
    """The four band windows for a judge level in the normal or strict group."""
    base = STRICT_BASE if pro else NORMAL_BASE
    offset = LEVEL_OFFSET_MS[judge]
    return [b + offset for b in base]


def real_time_windows(judge, pro, rate, compensated):
    """Windows as the judge actually sees them, in real time.

    Malody V compares `(this+44) * |delta|` against the window table, where
    `this+44` is 1.0 for a plain play and `1/playSpeed` when the speed object
    (`bmf+96`) is present. The three rate-changing Mods do **not** fill that object
    (they change speed through the Mod bits), so their windows stay as measured;
    the chart's own exact-speed path (Turbo) does fill it, so its windows shrink by
    the rate. That is the whole reason Turbo 1.2 and Dash 1.2 differ.
    """
    windows = base_windows(judge, pro)
    if compensated:
        return [w / rate for w in windows]
    return windows


def cell(judge, pro, rate, compensated):
    sigma = solve_sigma(real_time_windows(judge, pro, rate, compensated), MALODY_ACC_WEIGHTS)
    od, residual = solve_od(sigma)
    return {"sigma": sigma, "od": od, "residual": residual}


def solve_table():
    """(judge, pro, variant) -> cell.

    `NM` is a plain play. `DASH`/`RUSH`/`SLOW` are the Mod bits: faster/slower audio,
    windows untouched. `TURBO` is the exact-speed path at the same 1.2 rate as DASH,
    whose windows are compensated — so it must come out harder than DASH.
    """
    table = {}
    variants = (
        ("NM", 1.0, False),
        ("DASH", 1.2, False),
        ("RUSH", 1.5, False),
        ("SLOW", 0.8, False),
        ("TURBO", 1.2, True),
    )
    for judge in range(5):
        for pro in (False, True):
            for name, rate, compensated in variants:
                table[(judge, pro, name)] = {
                    "rate": rate,
                    "compensated": compensated,
                    **cell(judge, pro, rate, compensated),
                }
    return table


def checks(table):
    """Self-checks; a wrong table value must make this fail, not pass silently."""
    names = ("SLOW", "NM", "DASH", "RUSH", "TURBO")
    problems = []
    for judge in range(5):
        for pro in (False, True):
            for name in names:
                c = table[(judge, pro, name)]
                if c["residual"] > RESIDUAL_MAX_MS:
                    problems.append(
                        f"judge {judge} pro={pro} {name}: residual {c['residual']:.4f} ms"
                        f" exceeds {RESIDUAL_MAX_MS} ms (solve saturated at a bound)")
    # Pro must be harder than non-Pro at the same judge and variant (windows are narrower).
    for judge in range(5):
        for name in names:
            strict = table[(judge, True, name)]["od"]
            normal = table[(judge, False, name)]["od"]
            if not strict > normal:
                problems.append(
                    f"judge {judge} {name}: Pro OD {strict:.2f} is not above non-Pro {normal:.2f}")
    # The three Mods leave the windows alone, so their OD is the plain-play OD at the
    # same judge: only the rate (density) differs, and that is expressed by musicRate.
    for judge in range(5):
        for pro in (False, True):
            base = table[(judge, pro, "NM")]["od"]
            for name in ("DASH", "RUSH", "SLOW"):
                if abs(table[(judge, pro, name)]["od"] - base) > 1e-9:
                    problems.append(
                        f"judge {judge} pro={pro}: {name} OD {table[(judge, pro, name)]['od']:.2f}"
                        f" differs from the untouched-window NM OD {base:.2f}")
    # Turbo shares DASH's rate but compensates the windows => strictly harder.
    for judge in range(5):
        for pro in (False, True):
            turbo = table[(judge, pro, "TURBO")]["od"]
            dash = table[(judge, pro, "DASH")]["od"]
            if not turbo > dash:
                problems.append(
                    f"judge {judge} pro={pro}: TURBO OD {turbo:.2f} is not above DASH {dash:.2f}"
                    " (compensated windows must be stricter at the same rate)")
    return problems


def emit_md(table):
    for pro, label in ((False, "Pro OFF (normal group)"), (True, "Pro ON (strict group)")):
        print(f"\n### {label}\n")
        print("| judge | SLOW 0.8 | NM 1.0 | TURBO 1.2 | DASH 1.2 | RUSH 1.5 |")
        print("|---|---|---|---|---|---|")
        for judge in range(5):
            row = [
                f"{table[(judge, pro, n)]['od']:.2f}"
                for n in ("SLOW", "NM", "TURBO", "DASH", "RUSH")
            ]
            print(f"| {'ABCDE'[judge]} | " + " | ".join(row) + " |")


def emit_js(table):
    for pro, name in ((False, "NORMAL"), (True, "STRICT")):
        print(f"// {name} group")
        for judge in range(5):
            cells = ", ".join(
                f"{n}: {table[(judge, pro, n)]['od']:.2f}" for n in ("NM", "DASH", "RUSH", "SLOW")
            )
            print(f"    {judge}: Object.freeze({{ {cells} }}),")
        print()


JS_TABLE_REL = os.path.join("ManiaMapAnalyser by Leo_Black", "js", "app", "sources",
                            "odResolver.js")
# matches:  <judge>: Object.freeze({ NM: 4.87, DASH: 4.87, RUSH: 4.87, SLOW: 4.87 }),
ROW_RE = re.compile(
    r"^\s*(?P<judge>[0-4])\s*:\s*Object\.freeze\(\{\s*"
    r"NM\s*:\s*(?P<NM>-?\d+(?:\.\d+)?)\s*,\s*"
    r"DASH\s*:\s*(?P<DASH>-?\d+(?:\.\d+)?)\s*,\s*"
    r"RUSH\s*:\s*(?P<RUSH>-?\d+(?:\.\d+)?)\s*,\s*"
    r"SLOW\s*:\s*(?P<SLOW>-?\d+(?:\.\d+)?)\s*\}\)",
    re.MULTILINE,
)


def read_js_table(repo_root):
    """Extract the 20 stored cells from the page table without needing Node."""
    path = os.path.join(repo_root, JS_TABLE_REL)
    if not os.path.isfile(path):
        raise RuntimeError("page table not found: %s" % path)
    with open(path, "r", encoding="utf-8") as handle:
        source = handle.read()
    rows = list(ROW_RE.finditer(source))
    if len(rows) != 10:
        raise RuntimeError(
            "expected 10 judge rows (5 levels x 2 groups) in %s, found %d" % (path, len(rows)))
    # the first five rows are the normal group, the next five the strict group
    stored = {}
    for index, row in enumerate(rows):
        pro = index >= 5
        judge = int(row.group("judge"))
        for name in ("NM", "DASH", "RUSH", "SLOW"):
            stored[(judge, pro, name)] = float(row.group(name))
    return stored


def compare_with_js(table, stored):
    """Recomputed value vs stored value, per cell."""
    problems = []
    for judge in range(5):
        for pro in (False, True):
            for name in ("NM", "DASH", "RUSH", "SLOW"):
                want = table[(judge, pro, name)]["od"]
                got = stored.get((judge, pro, name))
                if got is None:
                    problems.append(f"judge {judge} pro={pro} {name}: missing from the page table")
                elif abs(want - got) > OD_MATCH_TOL:
                    problems.append(
                        f"judge {judge} pro={pro} {name}: page says {got:.2f},"
                        f" recomputation says {want:.2f}")
    return problems


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--emit-md", action="store_true")
    ap.add_argument("--emit-js", action="store_true")
    ap.add_argument("--check-js", action="store_true",
                    help="also compare the recomputation against the page table in "
                         "js/app/sources/odResolver.js; a wrong stored value exits non-zero")
    ap.add_argument("--repo-root",
                    default=os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))),
                    help="repository root (for --check-js); defaults to two levels above this file")
    args = ap.parse_args()

    table = solve_table()
    problems = checks(table)

    if args.emit_md:
        emit_md(table)
        return 0
    if args.emit_js:
        emit_js(table)
        return 0

    print("Malody V judge window -> equivalent osu!mania OD (sigma* at 96%)")
    print(f"normal group base {NORMAL_BASE} / strict group base {STRICT_BASE}\n")
    for pro, label in ((False, "Pro OFF"), (True, "Pro ON")):
        print(f"{label}:")
        print(f"  {'judge':6} {'SLOW':>7} {'NM':>7} {'TURBO':>7} {'DASH':>7} {'RUSH':>7}")
        for judge in range(5):
            cells = [table[(judge, pro, n)]["od"] for n in ("SLOW", "NM", "TURBO", "DASH", "RUSH")]
            print(f"  {'ABCDE'[judge]:6} " + " ".join(f"{c:7.2f}" for c in cells))
        print()

    print(f"cells: {len(table)}")

    if args.check_js:
        stored = read_js_table(args.repo_root)
        js_problems = compare_with_js(table, stored)
        if js_problems:
            print(f"\nPAGE TABLE MISMATCH ({len(js_problems)}):")
            for p in js_problems:
                print(f"  - {p}")
            problems += js_problems
        else:
            print(f"page table matches the recomputation ({len(stored)} stored cells,"
                  f" tolerance {OD_MATCH_TOL})")

    if problems:
        print(f"\nSELF-CHECK FAILED ({len(problems)}):")
        for p in problems:
            print(f"  - {p}")
        return 1
    print("\nSELF-CHECK PASSED: residuals within tolerance; Pro > non-Pro; the three Mods keep the"
          " plain-play OD; TURBO > DASH at equal rate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

// 转换器 golden 测试（合成样本 + 分类断言）。
//
// 运行：node --experimental-default-type=module tests/run-converter-tests.mjs
// （插件目录无 package.json type:module，Node 需以默认 ESM 模式加载 .js；
// 浏览器端无此问题。真实谱面样本仅本机私有，见 tests/fixtures/README。）

import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { readFileSync, existsSync, readdirSync } from "node:fs";
import { generateFixtures } from "./generate-fixtures.mjs";
import { convertSmSscToOsuText } from "../ManiaMapAnalyser by Leo_Black/js/parser/smSscToOsuConverter.js";
import { convertMcToOsuText } from "../ManiaMapAnalyser by Leo_Black/js/parser/mcToOsuConverter.js";
import { OsuFileParser } from "../ManiaMapAnalyser by Leo_Black/js/parser/osuFileParser.js";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const FIXTURES = join(__dirname, "fixtures");

// 确保合成样本存在（可随时重建；真实样本见 fixtures/README）。
generateFixtures(FIXTURES);

let passed = 0;
let failed = 0;
const failures = [];

function assert(cond, msg) {
    if (cond) {
        passed += 1;
    } else {
        failed += 1;
        failures.push(msg);
        console.error(`FAIL: ${msg}`);
    }
}

function loadFixture(name) {
    const p = join(FIXTURES, name);
    assert(existsSync(p), `fixture 缺失: ${name}`);
    return readFileSync(p, "utf-8");
}

function parseOsu(osuText) {
    const parser = new OsuFileParser(osuText);
    parser.process();
    return parser.getParsedData();
}

// 期望命中对象数（tap 数 + hold 数）与 hold 数，从解析结果统计。
function countNotes(parsed) {
    let holds = 0;
    for (const t of parsed.noteTypes || []) {
        if (t === 128) {
            holds += 1;
        }
    }
    return { total: (parsed.noteStarts || []).length, holds };
}

// ── 分类 1：无 STOPS 样本（BPM 点数/键数/note 数/时间戳精确断言）──
{
    const result = convertSmSscToOsuText({ text: loadFixture("sm-simple-4k.sm"), format: "sm" });
    assert(result.meta.keys === 4, `sm-simple keys=4, got ${result.meta.keys}`);
    assert(result.meta.noteCount === 5, `sm-simple noteCount=5, got ${result.meta.noteCount}`);
    assert(result.meta.holdCount === 1, `sm-simple holdCount=1, got ${result.meta.holdCount}`);
    assert(result.meta.bpmPoints === 1, `sm-simple bpmPoints=1, got ${result.meta.bpmPoints}`);
    const parsed = parseOsu(result.osuText);
    assert(parsed.columnCount === 4, `sm-simple parsed columnCount=4, got ${parsed.columnCount}`);
    const counts = countNotes(parsed);
    assert(counts.total === 5 && counts.holds === 1, `sm-simple parsed notes 5/1, got ${counts.total}/${counts.holds}`);
    assert(parsed.status !== "error", `sm-simple parsed status=${parsed.status}`);
    const starts = [...parsed.noteStarts].sort((a, b) => a - b);
    assert(starts[0] === 0 && starts[1] === 500 && starts[2] === 1000 && starts[3] === 1500,
        `sm-simple tap times 0/500/1000/1500, got ${starts.slice(0, 4).join(",")}`);
    assert(starts[4] === 2000, `sm-simple hold start 2000, got ${starts[4]}`);
    assert(parsed.noteEnds[4] === 4000, `sm-simple hold end 4000, got ${parsed.noteEnds[4]}`);
}

// ── 分类 2：STOPS 样本（烘焙偏移断言：stop 0.5 拍@120bpm = 250ms）──
{
    const result = convertSmSscToOsuText({ text: loadFixture("sm-stops-4k.sm"), format: "sm" });
    assert(result.meta.keys === 4, `sm-stops keys=4, got ${result.meta.keys}`);
    assert(result.meta.bpmPoints === 1, `sm-stops bpmPoints=1, got ${result.meta.bpmPoints}`);
    assert(result.osuText.includes("2250,"), `sm-stops 第二个 tap 烘焙为 2250ms（stop 250ms 后移）`);
    const parsed = parseOsu(result.osuText);
    assert(countNotes(parsed).total === 2, `sm-stops parsed notes=2, got ${countNotes(parsed).total}`);
    assert(parsed.status !== "error", `sm-stops parsed status=${parsed.status}`);
}

// ── 分类 3：7 列（vendor 列宽补丁：键数推导）──
{
    const result = convertSmSscToOsuText({ text: loadFixture("sm-7k.sm"), format: "sm" });
    assert(result.meta.keys === 7, `sm-7k keys=7, got ${result.meta.keys}`);
    assert(result.meta.noteCount === 4, `sm-7k noteCount=4, got ${result.meta.noteCount}`);
    const parsed = parseOsu(result.osuText);
    assert(parsed.columnCount === 7, `sm-7k parsed columnCount=7, got ${parsed.columnCount}`);
    assert(parsed.status !== "error", `sm-7k parsed status=${parsed.status}`);
}

// ── 分类 4：.ssc 多难度（按 difficulty 选择；默认第一块）──
{
    const byName = convertSmSscToOsuText({ text: loadFixture("ssc-multi.ssc"), format: "ssc", difficulty: "Medium" });
    assert(byName.meta.version.startsWith("difficult"), `ssc Medium(→difficult) 选择: version=${byName.meta.version}`);
    assert(byName.meta.keys === 4, `ssc Medium keys=4, got ${byName.meta.keys}`);
    assert(byName.meta.noteCount === 2, `ssc Medium noteCount=2, got ${byName.meta.noteCount}`);
    const defaultChart = convertSmSscToOsuText({ text: loadFixture("ssc-multi.ssc"), format: "ssc" });
    assert(defaultChart.meta.version.startsWith("expert"), `ssc 默认第一块(→expert): version=${defaultChart.meta.version}`);
    const parsed = parseOsu(byName.osuText);
    assert(parsed.columnCount === 4 && parsed.status !== "error", `ssc parsed OK, status=${parsed.status}`);
}

// ── 分类 5：.mc（SV 红线/tap/hold）──
{
    const result = convertMcToOsuText(loadFixture("mc-simple.mc"));
    assert(result.meta.keys === 4, `mc keys=4, got ${result.meta.keys}`);
    assert(result.meta.bpmPoints === 1, `mc bpmPoints=1, got ${result.meta.bpmPoints}`);
    assert(result.meta.svCount === 1, `mc svCount=1, got ${result.meta.svCount}`);
    assert(result.meta.noteCount === 2 && result.meta.holdCount === 1,
        `mc notes 2/1, got ${result.meta.noteCount}/${result.meta.holdCount}`);
    const svLine = result.osuText.split("\n").find((l) => l.includes(",-"));
    assert(svLine && svLine.startsWith("0,-66.666666") && /,4,1,0,0,0,0$/.test(svLine),
        `mc SV 红线 -66.666666（100/1.5）: ${svLine}`);
    const parsed = parseOsu(result.osuText);
    assert(parsed.columnCount === 4 && parsed.status !== "error", `mc parsed OK, status=${parsed.status}`);
    assert(countNotes(parsed).total === 2 && countNotes(parsed).holds === 1,
        `mc parsed notes 2/1, got ${countNotes(parsed).total}/${countNotes(parsed).holds}`);
}

// ── 分类 6：.mc LN 尾冲突修复（尾强制 < 下一条起始）──
{
    const result = convertMcToOsuText(loadFixture("mc-ln-collision.mc"));
    const parsed = parseOsu(result.osuText);
    const starts = parsed.noteStarts;
    const ends = parsed.noteEnds;
    const holdIdx = ends.findIndex((e) => e > 0);
    const nextIdx = starts.indexOf(2000);
    assert(holdIdx >= 0 && nextIdx >= 0, `mc-collision 存在 hold 与 2000ms tap`);
    assert(ends[holdIdx] === 1999 && starts[nextIdx] === 2000,
        `mc-collision 尾修复 1999/2000, got ${ends[holdIdx]}/${starts[nextIdx]}`);
}

// ── 边界：空谱面（无 note → 抛错而非崩溃）──
{
    let threw = false;
    try {
        convertSmSscToOsuText({ text: loadFixture("sm-rests-only.sm"), format: "sm" });
    } catch {
        threw = true;
    }
    assert(threw, "sm-rests-only 应抛出「未找到难度块」错误");
}

// ── 真实样本冒烟（仅本机私有目录存在时运行；见 fixtures/README）──
{
    const realDir = join(FIXTURES, "real");
    if (existsSync(realDir)) {
        const files = readdirSync(realDir).filter((f) => f.endsWith(".sm") || f.endsWith(".ssc") || f.endsWith(".mc"));
        let converted = 0;
        for (const f of files) {
            try {
                const text = readFileSync(join(realDir, f), "utf-8");
                const result = f.endsWith(".mc")
                    ? convertMcToOsuText(text)
                    : convertSmSscToOsuText({ text, format: f.endsWith(".ssc") ? "ssc" : "sm" });
                const parsed = parseOsu(result.osuText);
                assert(parsed.status !== "error" && result.meta.keys > 0,
                    `real ${f}: keys=${result.meta.keys} status=${parsed.status}`);
                converted += 1;
            } catch (e) {
                assert(false, `real ${f}: ${e.message}`);
            }
        }
        console.log(`real fixtures: ${converted}/${files.length} converted`);
    } else {
        console.log("real fixtures: dir missing (skip) — 按 docs/pipeline/converters.md 放置本机真实样本");
    }
}

console.log(`\nSMOKE: ${passed}/${passed + failed} PASS`);
if (failed > 0) {
    process.exit(1);
}
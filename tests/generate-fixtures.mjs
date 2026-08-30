// 合成谱面样本生成器（转换器测试用）。
//
// 真实谱面仅限本机私有（tests/fixtures 已 gitignored）；仓库内测试资产只含
// 本生成器产出的合成样本与 run-converter-tests.mjs 的期望断言。

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const SM_SIMPLE_4K = `#TITLE:Simple Test
#ARTIST:Tester
#BPMS:0.000=120.000
#STOPS:
#OFFSET:0
#NOTES:
dance-single:
author:
Hard:
8:
0,0,0,0,0:
1000
0010
0001
1000
,
2000
0000
0000
3000
;
`;

const SM_STOPS_4K = `#TITLE:Stops Test
#ARTIST:Tester
#BPMS:0.000=120.000
#STOPS:0.500=0.500
#OFFSET:0
#NOTES:
dance-single:
author:
Hard:
8:
0,0,0,0,0:
1000
0000
0000
0000
,
1000
0000
0000
0000
;
`;

const SM_7K = `#TITLE:Seven Keys
#ARTIST:Tester
#BPMS:0.000=120.000
#STOPS:
#OFFSET:0
#NOTES:
dance-single:
author:
Hard:
8:
0,0,0,0,0:
1000000
0100000
0001000
0000001
;
`;

const SSC_MULTI = `#TITLE:SSC Multi;
#ARTIST:Tester;
#OFFSET:0;
#BPMS:0.000=120.000;
#STOPS:;
#NOTEDATA:;
#STEPSTYPE:dance-single
#DIFFICULTY:Hard
#METER:12
#NOTES:
1000
0100
0010
0001
;
#NOTEDATA:;
#STEPSTYPE:dance-single
#DIFFICULTY:Medium
#METER:8
#NOTES:
1000
0000
0100
0000
;
`;

const MC_SIMPLE = JSON.stringify({
    meta: {
        mode: 0,
        mode_ext: { column: 4 },
        song: { title: "MC Test", artist: "Tester" },
        creator: "u",
        version: "Lv.10",
        preview: -1,
    },
    time: [{ bpm: 120, beat: [0, 0, 1] }],
    note: [
        { type: 0, column: 0, beat: [0, 0, 1] },
        { type: 0, column: 1, beat: [1, 0, 1], endbeat: [2, 0, 1] },
    ],
    effect: [{ beat: [0, 0, 1], scroll: 1.5 }],
});

const MC_LN_COLLISION = JSON.stringify({
    meta: {
        mode: 0,
        mode_ext: { column: 4 },
        song: { title: "LN Collision", artist: "Tester" },
        creator: "u",
        version: "Lv.10",
        preview: -1,
    },
    time: [{ bpm: 120, beat: [0, 0, 1] }],
    note: [
        { type: 0, column: 0, beat: [0, 0, 1], endbeat: [4, 0, 1] },
        { type: 0, column: 0, beat: [4, 0, 1] },
    ],
    effect: [],
});

const SM_RESTS_ONLY = `#TITLE:Empty Chart
#ARTIST:Tester
#BPMS:0.000=120.000
#STOPS:
#OFFSET:0
#NOTES:
dance-single:
author:
8:
0,0,0,0,0:
0000
0000
0000
0000
;
`;

/**
 * 生成全部合成样本到指定目录。
 * @param {string} outDir 输出目录
 */
export function generateFixtures(outDir) {
    mkdirSync(outDir, { recursive: true });
    const files = {
        "sm-simple-4k.sm": SM_SIMPLE_4K,
        "sm-stops-4k.sm": SM_STOPS_4K,
        "sm-7k.sm": SM_7K,
        "ssc-multi.ssc": SSC_MULTI,
        "mc-simple.mc": MC_SIMPLE,
        "mc-ln-collision.mc": MC_LN_COLLISION,
        "sm-rests-only.sm": SM_RESTS_ONLY,
    };
    for (const [name, content] of Object.entries(files)) {
        writeFileSync(join(outDir, name), content, "utf-8");
    }
    return Object.keys(files).length;
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
    const out = join(process.cwd(), "tests", "fixtures");
    const n = generateFixtures(out);
    console.log(`generated ${n} fixtures into ${out}`);
}
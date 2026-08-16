// Patch MinaCalc WASM binaries: raise the internal skillset (SSR) cap from
// 40.0 to 100.0 by replacing `f32.const 40.0` with `f32.const 100.0`.
//
// Why: MinaCalc clamps skill values at 40.0, flattening MSD results for
// ultra-hard charts (high-speed jacks / high density). Replacing the constant
// is an equal-length 5-byte -> 5-byte edit, so wasm section offsets, function
// tables and import/export tables stay intact.
//
// Byte patterns (little-endian IEEE754):
//   f32.const 40.0  = 43 00 00 20 42
//   f32.const 100.0 = 43 00 00 c8 42
//
// Safety: original binaries are preserved under the local `backup/` folder
// (gitignored) before patching; the shipped `.wasm` files are git-tracked, so
// pristine bytes are also recoverable from git history. The script is
// idempotent: files with no remaining 40.0 constant are reported as already
// patched and left untouched.
//
// Usage: node tools/patch-minaclac-msd-cap.mjs
// (run from the repository root or anywhere; paths resolve relative to this file)
import { readdirSync, readFileSync, writeFileSync, mkdirSync, existsSync, copyFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const TOOLS_DIR = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(TOOLS_DIR, "..");
const VERSIONS_DIR = join(REPO_ROOT, "ManiaMapAnalyser by Leo_Black", "js", "ett", "versions");

const OLD = [0x43, 0x00, 0x00, 0x20, 0x42]; // f32.const 40.0
const NEW = [0x43, 0x00, 0x00, 0xc8, 0x42]; // f32.const 100.0

function countPattern(buf, pattern) {
    let count = 0;
    for (let i = 0; i + pattern.length <= buf.length; i += 1) {
        let match = true;
        for (let j = 0; j < pattern.length; j += 1) {
            if (buf[i + j] !== pattern[j]) {
                match = false;
                break;
            }
        }
        if (match) {
            count += 1;
            i += pattern.length - 1; // no self-overlap possible, but keep it tight
        }
    }
    return count;
}

function patch(buf, oldPattern, newPattern) {
    const out = Buffer.from(buf);
    let count = 0;
    for (let i = 0; i + oldPattern.length <= out.length; i += 1) {
        let match = true;
        for (let j = 0; j < oldPattern.length; j += 1) {
            if (out[i + j] !== oldPattern[j]) {
                match = false;
                break;
            }
        }
        if (match) {
            for (let j = 0; j < newPattern.length; j += 1) {
                out[i + j] = newPattern[j];
            }
            count += 1;
            i += oldPattern.length - 1;
        }
    }
    return { out, count };
}

const wasmFiles = readdirSync(VERSIONS_DIR)
    .filter((name) => name.endsWith(".wasm"))
    .sort();

if (wasmFiles.length === 0) {
    console.error(`No .wasm files found under ${VERSIONS_DIR}`);
    process.exit(1);
}

const backupStamp = new Date().toISOString().replace(/[:.]/g, "-");
const backupDir = join(REPO_ROOT, "backup", `msd-cap-patch-${backupStamp}`);

let patchedAny = false;
for (const file of wasmFiles) {
    const path = join(VERSIONS_DIR, file);
    const buf = readFileSync(path);
    const oldCount = countPattern(buf, OLD);

    if (oldCount === 0) {
        console.log(`${file}: no f32.const 40.0 found (already patched or no cap)`);
        continue;
    }

    if (!existsSync(backupDir)) {
        mkdirSync(backupDir, { recursive: true });
    }
    copyFileSync(path, join(backupDir, file));

    const { out, count } = patch(buf, OLD, NEW);
    writeFileSync(path, out);
    patchedAny = true;

    const new100 = countPattern(out, NEW);
    console.log(`${file}: patched ${count} occurrence(s) 40.0 -> 100.0 (100.0 total now: ${new100})`);
}

if (!patchedAny) {
    console.log("Nothing to patch.");
} else {
    console.log(`Originals backed up to ${backupDir}`);
}

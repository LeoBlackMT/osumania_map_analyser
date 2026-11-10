// random.js —— osu!lazer ManiaModRandom(RD) 的列置换（DOM-free）。
//
// 复现 .NET seeded System.Random（Knuth subtractive generator）与
// ManiaModRandom.ApplyToBeatmap：
//   new Random((int)seed)
//   Enumerable.Range(0, columns).OrderBy(_ => rng.Next())
// 返回 shuffledColumns：oldColumn -> newColumn。

class CSharpRandom {
    constructor(seed) {
        seed |= 0;
        const subtraction = seed === -2147483648 ? 2147483647 : Math.abs(seed);
        let mj = (161803398 - subtraction) | 0;
        this.seedArray = new Array(56).fill(0);
        this.seedArray[55] = mj;
        let mk = 1;
        let ii = 0;
        for (let i = 1; i < 55; i += 1) {
            ii += 21;
            if (ii >= 55) ii -= 55;
            this.seedArray[ii] = mk;
            mk = (mj - mk) | 0;
            if (mk < 0) mk += 2147483647;
            mj = this.seedArray[ii];
        }
        for (let k = 1; k < 5; k += 1) {
            for (let i = 1; i < 56; i += 1) {
                let n = i + 30;
                if (n >= 55) n -= 55;
                this.seedArray[i] = (this.seedArray[i] - this.seedArray[1 + n]) | 0;
                if (this.seedArray[i] < 0) this.seedArray[i] += 2147483647;
            }
        }
        this.inext = 0;
        this.inextp = 21;
    }

    next() {
        let a = this.inext + 1;
        if (a >= 56) a = 1;
        let b = this.inextp + 1;
        if (b >= 56) b = 1;
        let value = this.seedArray[a] - this.seedArray[b];
        if (value === 2147483647) value -= 1;
        if (value < 0) value += 2147483647;
        this.seedArray[a] = value;
        this.inext = a;
        this.inextp = b;
        return value;
    }
}

/** ManiaModRandom 的列置换：返回 shuffledColumns[oldColumn] = newColumn。 */
export function maniaRandomColumnMap(keyCount, seed) {
    const count = Number(keyCount);
    if (!Number.isInteger(count) || count <= 0 || count > 32) {
        return null;
    }
    const rng = new CSharpRandom(seed | 0);
    return Array.from({ length: count }, (_, column) => ({ column, key: rng.next() }))
        .sort((a, b) => a.key - b.key || a.column - b.column)
        .map((entry) => entry.column);
}

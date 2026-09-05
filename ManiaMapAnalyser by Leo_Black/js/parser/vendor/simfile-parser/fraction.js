/*********
 * This is a port of the bare minimum functionality from the 'fraction.js'
 * npm package to suit the needs of dealing with fractional beat offsets.
 * The original package has types written in a strange way that doesn't make
 * typescript happy when in pure ESM mode.
 *
 * This and the original implementation are available under the MIT license.
 * See: https://github.com/infusion/Fraction.js/blob/master/fraction.js
 *********/
// eslint-disable-next-line jsdoc/require-jsdoc
function gcd(a, b) {
    if (!a)
        return b;
    if (!b)
        return a;
    while (true) {
        a %= b;
        if (!a)
            return b;
        b %= a;
        if (!b)
            return a;
    }
}
export class Fraction {
    n;
    d;
    constructor(n, d = 1) {
        this.n = n;
        this.d = d;
    }
    add(f) {
        if (f.d === this.d) {
            return new Fraction(this.n + f.n, this.d);
        }
        return new Fraction(this.n * f.d + this.d * f.n, this.d * f.d).simplify();
    }
    mod(f) {
        return new Fraction((f.d * this.n) % (f.n * this.d), f.d * this.d);
    }
    simplify() {
        const reduceBy = gcd(this.n, this.d);
        return new Fraction(this.n / reduceBy, this.d / reduceBy);
    }
    toString() {
        return (this.n / this.d).toString();
    }
}
//# sourceMappingURL=fraction.js.map
// Shared lane-mapping util for the two .osu parsers. osuFileParser feeds
// integer x (stringToInt) here; for integer x in [0, 512] both the
// `(x * keys) / 512` and `(x / 512.0) * keys` forms evaluate to the exact
// same dyadic rational, so the shared trunc-then-clamp formula is
// bit-identical for both callers.
export function xToColumn(x, keys) {
    let col = Math.trunc((x / 512.0) * keys);
    if (col < 0) col = 0;
    if (col > keys - 1) col = keys - 1;
    return col;
}

# simfile-parser (vendored)

Vendored from [`noahm/simfile-parser`](https://github.com/noahm/simfile-parser) v0.9.0
(npm package `simfile-parser`, MIT license).

Used under the MIT License:

> MIT License
>
> Copyright (c) 2024 Noah Manneschmidt
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all
> copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
> SOFTWARE.

## Files

- `fraction.js`, `util.js`, `parsers/parseSm.js`, `parsers/parseSsc.js` — copied
  verbatim from the package `dist/` output (browser-safe: no Node imports).

## Local patches

Marked with `[vendor patch]` comments:

- `parsers/parseSm.js`, `parsers/parseSsc.js` — `trimNoteLine` no longer
  truncates rows to 4/8 columns: Etterna keyboard charts (6K/7K) may declare
  `dance-single`/`dance-double` while carrying wider note rows, and truncation
  would silently drop columns (column count is derived by the consumer from the
  full row width).
- `parsers/parseSm.js` — charts whose stepstype is neither `dance-single` nor
  `dance-double` are kept instead of skipped (nonstandard keyboard stepstypes).
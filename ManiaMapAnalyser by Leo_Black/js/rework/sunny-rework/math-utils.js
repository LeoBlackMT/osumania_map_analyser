// ---- bisect ----
export function bisectLeft(arr, x, lo, hi) {
  lo = lo || 0;
  hi = hi === undefined ? arr.length : hi;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (arr[mid] < x) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export function bisectRight(arr, x, lo, hi) {
  lo = lo || 0;
  hi = hi === undefined ? arr.length : hi;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (arr[mid] <= x) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

export function cumsum(arr) {
  const result = new Array(arr.length);
  let s = 0;
  for (let i = 0; i < arr.length; i++) {
    s += arr[i];
    result[i] = s;
  }
  return result;
}

// ---- interpolation ----
export function interpValues(newX, oldX, oldVals) {
  const result = new Array(newX.length);
  let j = 0;
  for (let i = 0; i < newX.length; i++) {
    const x = newX[i];
    if (x <= oldX[0]) {
      result[i] = oldVals[0];
      continue;
    }
    if (x >= oldX[oldX.length - 1]) {
      result[i] = oldVals[oldVals.length - 1];
      continue;
    }
    while (j < oldX.length - 1 && oldX[j + 1] < x) j++;
    const x0 = oldX[j], x1 = oldX[j + 1];
    const y0 = oldVals[j], y1 = oldVals[j + 1];
    const t = (x - x0) / (x1 - x0);
    result[i] = y0 + t * (y1 - y0);
  }
  return result;
}

export function stepInterp(newX, oldX, oldVals) {
  const result = new Array(newX.length);
  for (let i = 0; i < newX.length; i++) {
    const idx = Math.max(0, Math.min(oldVals.length - 1, bisectRight(oldX, newX[i]) - 1));
    result[i] = oldVals[idx];
  }
  return result;
}

// ---- cumulative sum utilities ----
export function cumulativeSum(x, f) {
  const F = new Array(x.length);
  F[0] = 0;
  for (let i = 1; i < x.length; i++) {
    F[i] = F[i - 1] + f[i - 1] * (x[i] - x[i - 1]);
  }
  return F;
}

export function queryCumsum(q, x, F, f) {
  if (q <= x[0]) return 0.0;
  if (q >= x[x.length - 1]) return F[F.length - 1];
  let lo = 0, hi = x.length;
  while (lo < hi) {
    const mid = (lo + hi) >>> 1;
    if (x[mid] < q) lo = mid + 1;
    else hi = mid;
  }
  const i = lo - 1;
  return F[i] + f[i] * (q - x[i]);
}

export function smoothOnCorners(x, f, window, scale, mode) {
  scale = scale === undefined ? 1.0 : scale;
  mode = mode || 'sum';
  const F = cumulativeSum(x, f);
  const g = new Array(f.length);
  for (let i = 0; i < x.length; i++) {
    const s = x[i];
    const a = Math.max(s - window, x[0]);
    const b = Math.min(s + window, x[x.length - 1]);
    const val = queryCumsum(b, x, F, f) - queryCumsum(a, x, F, f);
    if (mode === 'avg') {
      g[i] = (b - a) > 0 ? val / (b - a) : 0.0;
    } else {
      g[i] = scale * val;
    }
  }
  return g;
}

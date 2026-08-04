// Compatibility target: Sunny Rework 2025-04-15. See docs/PROVENANCE.md.

import {
  bisectLeft,
  bisectRight,
  interpValues,
  smoothOnCorners,
  stepInterp
} from './math-utils.js';

export const SUNNY_REWORK_TARGET = '2025-04-15';

var SUNNY_CLEAN_CROSS_WEIGHTS = [
  null,
  [0.075, 0.075],
  [0.125, 0.05, 0.125],
  [0.125, 0.125, 0.125, 0.125],
  [0.175, 0.25, 0.05, 0.25, 0.175],
  [0.175, 0.25, 0.175, 0.175, 0.25, 0.175],
  [0.225, 0.35, 0.25, 0.05, 0.25, 0.35, 0.225],
  [0.225, 0.35, 0.25, 0.225, 0.225, 0.25, 0.35, 0.225],
  [0.275, 0.45, 0.35, 0.25, 0.05, 0.25, 0.35, 0.45, 0.275],
  [0.275, 0.45, 0.35, 0.25, 0.275, 0.275, 0.25, 0.35, 0.45, 0.275],
  [0.325, 0.55, 0.45, 0.35, 0.25, 0.05, 0.25, 0.35, 0.45, 0.55, 0.325]
];

function sunnyCleanZero() {
  return { sr: 0, spikiness: 0, switches: 0.75, variety: 0 };
}

function sunnyCleanUniqueSorted(values) {
  return Array.from(new Set(values)).sort(function (a, b) { return a - b; });
}

function sunnyCleanNormalize(parsed, mod) {
  var rate = mod === 'DT' ? 2 / 3 : mod === 'HT' ? 4 / 3 : 1;
  var columns = parsed[1] || [];
  var starts = parsed[2] || [];
  var ends = parsed[3] || [];
  var notes = [];

  for (var i = 0; i < columns.length; i++) {
    var end = Number(ends[i]);
    notes.push({
      column: Number(columns[i]),
      start: Math.trunc(Number(starts[i]) * rate),
      end: end < 0 ? -1 : Math.trunc(end * rate)
    });
  }
  notes.sort(function (a, b) { return a.start - b.start || a.column - b.column; });
  return notes;
}

function sunnyCleanHitLeniency(od) {
  var value = 0.3 * Math.sqrt((64.5 - Math.ceil(od * 3)) / 500);
  return Math.min(value, 0.6 * (value - 0.09) + 0.09);
}

function sunnyCleanCorners(notes, duration) {
  var boundaries = [];
  for (var i = 0; i < notes.length; i++) {
    boundaries.push(notes[i].start);
    if (notes[i].end >= 0) boundaries.push(notes[i].end);
  }
  boundaries = sunnyCleanUniqueSorted(boundaries);

  var base = [0, duration];
  var unevenness = [0, duration];
  for (i = 0; i < boundaries.length; i++) {
    var time = boundaries[i];
    base.push(time, time + 501, time - 499, time + 1);
    unevenness.push(time, time + 1000, time - 1000);
  }
  base = sunnyCleanUniqueSorted(base.filter(function (time) { return time >= 0 && time <= duration; }));
  unevenness = sunnyCleanUniqueSorted(unevenness.filter(function (time) { return time >= 0 && time <= duration; }));
  return {
    base: base,
    unevenness: unevenness,
    all: sunnyCleanUniqueSorted(base.concat(unevenness))
  };
}

function sunnyCleanGroupColumns(notes, keys) {
  var result = Array.from({ length: keys }, function () { return []; });
  for (var i = 0; i < notes.length; i++) result[notes[i].column].push(notes[i]);
  return result;
}

function sunnyCleanKeyUsage(notes, keys, duration, corners) {
  var usage = Array.from({ length: keys }, function () { return new Uint8Array(corners.length); });
  for (var i = 0; i < notes.length; i++) {
    var note = notes[i];
    var start = Math.max(note.start - 150, 0);
    var end = note.end < 0 ? note.start + 150 : Math.min(note.end + 150, duration - 1);
    var left = bisectLeft(corners, start);
    var right = bisectLeft(corners, end);
    for (var index = left; index < right; index++) usage[note.column][index] = 1;
  }
  return usage;
}

function sunnyCleanActiveColumns(usage) {
  var result = Array.from({ length: usage[0].length }, function () { return []; });
  for (var index = 0; index < result.length; index++) {
    for (var column = 0; column < usage.length; column++) {
      if (usage[column][index]) result[index].push(column);
    }
  }
  return result;
}

function sunnyCleanAnchor(notes, keys, duration, corners) {
  var usage = Array.from({ length: keys }, function () { return new Float64Array(corners.length); });
  for (var i = 0; i < notes.length; i++) {
    var note = notes[i];
    var start = Math.max(note.start, 0);
    var end = note.end < 0 ? note.start : Math.min(note.end, duration - 1);
    var left400 = bisectLeft(corners, start - 400);
    var left = bisectLeft(corners, start);
    var right = bisectLeft(corners, end);
    var right400 = bisectLeft(corners, end + 400);

    for (var index = left; index < right; index++) {
      usage[note.column][index] += 3.75 + Math.min(end - start, 1500) / 150;
    }
    for (index = left400; index < left; index++) {
      var before = corners[index] - start;
      usage[note.column][index] += 3.75 - 3.75 / 160000 * before * before;
    }
    for (index = right; index < right400; index++) {
      var after = corners[index] - end;
      usage[note.column][index] += 3.75 - 3.75 / 160000 * after * after;
    }
  }

  var anchor = new Float64Array(corners.length);
  for (index = 0; index < corners.length; index++) {
    var counts = [];
    for (var column = 0; column < keys; column++) {
      if (usage[column][index] !== 0) counts.push(usage[column][index]);
    }
    counts.sort(function (a, b) { return b - a; });
    var walk = 0;
    var maximumWalk = 0;
    for (i = 0; i < counts.length - 1; i++) {
      var ratio = counts[i + 1] / counts[i];
      walk += counts[i] * (1 - 4 * Math.pow(0.5 - ratio, 2));
      maximumWalk += counts[i];
    }
    var raw = maximumWalk > 0 ? walk / maximumWalk : 0;
    anchor[index] = 1 + Math.min(raw - 0.18, 5 * Math.pow(raw - 0.22, 3));
  }
  return anchor;
}

function sunnyCleanJ(keys, x, byColumn, corners) {
  var deltas = Array.from({ length: keys }, function () {
    var row = new Float64Array(corners.length);
    row.fill(1e9);
    return row;
  });
  var smoothed = new Array(keys);

  for (var column = 0; column < keys; column++) {
    var raw = new Float64Array(corners.length);
    var notes = byColumn[column];
    for (var i = 0; i < notes.length - 1; i++) {
      var left = bisectLeft(corners, notes[i].start);
      var right = bisectLeft(corners, notes[i + 1].start);
      if (left >= right) continue;
      var delta = 0.001 * (notes[i + 1].start - notes[i].start);
      var nerf = 1 - 0.00007 * Math.pow(0.15 + Math.abs(delta - 0.08), -4);
      var value = nerf / (delta * (delta + 0.11 * Math.pow(x, 0.25)));
      for (var index = left; index < right; index++) {
        raw[index] = value;
        deltas[column][index] = delta;
      }
    }
    smoothed[column] = smoothOnCorners(corners, raw, 500, 0.001, 'sum');
  }

  var result = new Float64Array(corners.length);
  for (index = 0; index < corners.length; index++) {
    var numerator = 0;
    var denominator = 0;
    for (column = 0; column < keys; column++) {
      var weight = 1 / deltas[column][index];
      numerator += Math.pow(Math.max(smoothed[column][index], 0), 5) * weight;
      denominator += weight;
    }
    result[index] = Math.pow(numerator / Math.max(1e-9, denominator), 0.2);
  }
  return { values: result, deltas: deltas };
}

function sunnyCleanX(keys, x, byColumn, activeColumns, corners) {
  var coefficients = SUNNY_CLEAN_CROSS_WEIGHTS[keys];
  var normal = Array.from({ length: keys + 1 }, function () { return new Float64Array(corners.length); });
  var fast = Array.from({ length: keys + 1 }, function () { return new Float64Array(corners.length); });

  for (var boundary = 0; boundary <= keys; boundary++) {
    var notes;
    if (boundary === 0) notes = byColumn[0];
    else if (boundary === keys) notes = byColumn[keys - 1];
    else notes = byColumn[boundary - 1].concat(byColumn[boundary]).sort(function (a, b) { return a.start - b.start; });

    for (var i = 1; i < notes.length; i++) {
      var left = bisectLeft(corners, notes[i - 1].start);
      var right = bisectLeft(corners, notes[i].start);
      if (left >= right) continue;
      var delta = 0.001 * (notes[i].start - notes[i - 1].start);
      var value = 0.16 * Math.pow(Math.max(x, delta), -2);
      var leftAbsent = activeColumns[left].indexOf(boundary - 1) < 0 && activeColumns[right].indexOf(boundary - 1) < 0;
      var rightAbsent = activeColumns[left].indexOf(boundary) < 0 && activeColumns[right].indexOf(boundary) < 0;
      if (leftAbsent || rightAbsent) value *= 1 - coefficients[boundary];
      var fastValue = Math.max(0, 0.4 * Math.pow(Math.max(delta, 0.06, 0.75 * x), -2) - 80);
      for (var index = left; index < right; index++) {
        normal[boundary][index] = value;
        fast[boundary][index] = fastValue;
      }
    }
  }

  var combined = new Float64Array(corners.length);
  for (index = 0; index < corners.length; index++) {
    var valueNormal = 0;
    var valueFast = 0;
    for (boundary = 0; boundary <= keys; boundary++) valueNormal += normal[boundary][index] * coefficients[boundary];
    for (boundary = 0; boundary < keys; boundary++) {
      valueFast += Math.sqrt(fast[boundary][index] * coefficients[boundary]
        * fast[boundary + 1][index] * coefficients[boundary + 1]);
    }
    combined[index] = valueNormal + valueFast;
  }
  return smoothOnCorners(corners, combined, 500, 0.001, 'sum');
}

function sunnyCleanLnBodies(longNotes, duration) {
  var changes = new Map();
  function add(time, amount) { changes.set(time, (changes.get(time) || 0) + amount); }
  for (var i = 0; i < longNotes.length; i++) {
    var note = longNotes[i];
    var first = Math.min(note.start + 60, note.end);
    var second = Math.min(note.start + 120, note.end);
    add(first, 1.3);
    add(second, -0.3);
    add(note.end, -1);
  }
  var points = sunnyCleanUniqueSorted([0, duration].concat(Array.from(changes.keys())));
  var values = new Float64Array(Math.max(0, points.length - 1));
  var sums = new Float64Array(points.length);
  var current = 0;
  for (i = 0; i < points.length - 1; i++) {
    current += changes.get(points[i]) || 0;
    values[i] = Math.min(current, 2.5 + 0.5 * current);
    sums[i + 1] = sums[i] + (points[i + 1] - points[i]) * values[i];
  }
  return { points: points, values: values, sums: sums };
}

function sunnyCleanLnSum(start, end, bodies) {
  var left = bisectRight(bodies.points, start) - 1;
  var right = bisectRight(bodies.points, end) - 1;
  if (left === right) return (end - start) * bodies.values[left];
  return (bodies.points[left + 1] - start) * bodies.values[left]
    + bodies.sums[right] - bodies.sums[left + 1]
    + (end - bodies.points[right]) * bodies.values[right];
}

function sunnyCleanP(notes, x, anchor, corners, bodies) {
  var raw = new Float64Array(corners.length);
  for (var i = 0; i < notes.length - 1; i++) {
    var start = notes[i].start;
    var end = notes[i + 1].start;
    var elapsed = end - start;
    var left = bisectLeft(corners, start);
    if (elapsed === 0) {
      raw[left] += 1000 * Math.pow(0.02 * (4 / x - 24), 0.25);
      continue;
    }
    var right = bisectLeft(corners, end);
    if (left >= right) continue;
    var delta = 0.001 * elapsed;
    var lnValue = 1 + 0.006 * sunnyCleanLnSum(start, end, bodies);
    var streamRate = 7.5 / delta;
    var streamValue = streamRate > 160 && streamRate < 360
      ? 1 + 0.00000017 * (streamRate - 160) * Math.pow(streamRate - 360, 2)
      : 1;
    var inner = delta < 2 * x / 3
      ? 1 - 24 / x * Math.pow(delta - x / 2, 2)
      : 1 - 24 / x * Math.pow(x / 6, 2);
    var increment = Math.pow(0.08 / x * inner, 0.25) * Math.max(streamValue, lnValue) / delta;
    for (var index = left; index < right; index++) {
      raw[index] += Math.min(increment * anchor[index], Math.max(increment, increment * 2 - 10));
    }
  }
  return smoothOnCorners(corners, raw, 500, 0.001, 'sum');
}

function sunnyCleanA(keys, activeColumns, deltas, aCorners, baseCorners) {
  var differences = Array.from({ length: Math.max(0, keys - 1) }, function () { return new Float64Array(baseCorners.length); });
  for (var index = 0; index < baseCorners.length; index++) {
    var columns = activeColumns[index];
    for (var i = 0; i < columns.length - 1; i++) {
      var left = columns[i];
      var right = columns[i + 1];
      differences[left][index] = Math.abs(deltas[left][index] - deltas[right][index])
        + 0.4 * Math.max(0, Math.max(deltas[left][index], deltas[right][index]) - 0.11);
    }
  }

  var raw = new Float64Array(aCorners.length);
  raw.fill(1);
  for (index = 0; index < aCorners.length; index++) {
    var baseIndex = Math.min(bisectLeft(baseCorners, aCorners[index]), baseCorners.length - 1);
    columns = activeColumns[baseIndex];
    for (i = 0; i < columns.length - 1; i++) {
      left = columns[i];
      right = columns[i + 1];
      var delta = differences[left][baseIndex];
      var maximum = Math.max(deltas[left][baseIndex], deltas[right][baseIndex]);
      if (delta < 0.02) raw[index] *= Math.min(0.75 + 0.5 * maximum, 1);
      else if (delta < 0.07) raw[index] *= Math.min(0.65 + 5 * delta + 0.5 * maximum, 1);
    }
  }
  return smoothOnCorners(aCorners, raw, 250, 1, 'avg');
}

function sunnyCleanR(longNotes, byColumn, x, corners) {
  var tails = longNotes.slice().sort(function (a, b) { return a.end - b.end; });
  var indices = new Float64Array(tails.length);
  for (var i = 0; i < tails.length; i++) {
    var note = tails[i];
    var columnNotes = byColumn[note.column];
    var nextStart = Number.MAX_SAFE_INTEGER;
    for (var j = 0; j < columnNotes.length; j++) {
      if (columnNotes[j] === note && j + 1 < columnNotes.length) {
        nextStart = columnNotes[j + 1].start;
        break;
      }
    }
    var headSpacing = 0.001 * Math.abs(note.end - note.start - 80) / x;
    var tailSpacing = 0.001 * Math.abs(nextStart - note.end - 80) / x;
    indices[i] = 2 / (2 + Math.exp(-5 * (headSpacing - 0.75)) + Math.exp(-5 * (tailSpacing - 0.75)));
  }

  var raw = new Float64Array(corners.length);
  for (i = 0; i < tails.length - 1; i++) {
    var left = bisectLeft(corners, tails[i].end);
    var right = bisectLeft(corners, tails[i + 1].end);
    if (left >= right) continue;
    var releaseDelta = 0.001 * (tails[i + 1].end - tails[i].end);
    var value = 0.08 * Math.pow(releaseDelta, -0.5) / x * (1 + 0.8 * (indices[i] + indices[i + 1]));
    for (var index = left; index < right; index++) raw[index] = value;
  }
  return { values: smoothOnCorners(corners, raw, 500, 0.001, 'sum'), tails: tails };
}

function sunnyCleanCounts(notes, keys, usage, corners) {
  var heads = notes.map(function (note) { return note.start; });
  var counts = new Float64Array(corners.length);
  var active = new Float64Array(corners.length);
  for (var index = 0; index < corners.length; index++) {
    counts[index] = bisectLeft(heads, corners[index] + 500) - bisectLeft(heads, corners[index] - 500);
    var activeCount = 0;
    for (var column = 0; column < keys; column++) activeCount += usage[column][index];
    active[index] = Math.max(activeCount, 1);
  }
  return { counts: counts, active: active };
}

function sunnyCleanRao(values, logIterations) {
  if (!values.length) return 0;
  var counts = new Map();
  for (var i = 0; i < values.length; i++) counts.set(values[i], (counts.get(values[i]) || 0) + 1);
  var categories = Array.from(counts.keys());
  var result = 0;
  for (i = 0; i < categories.length; i++) {
    for (var j = 0; j < categories.length; j++) {
      var distance = Math.abs(categories[i] - categories[j]);
      for (var iteration = 0; iteration < logIterations; iteration++) distance = Math.log1p(distance);
      result += counts.get(categories[i]) / values.length * counts.get(categories[j]) / values.length * distance;
    }
  }
  return result;
}

function sunnyCleanVariety(notes, byColumn) {
  var headGaps = [];
  for (var i = 0; i < notes.length - 1; i++) headGaps.push(notes[i + 1].start - notes[i].start);
  var sortedByTail = notes.slice().sort(function (a, b) { return a.end - b.end; });
  var tailGaps = [];
  for (i = 0; i < sortedByTail.length - 1; i++) tailGaps.push(sortedByTail[i + 1].end - sortedByTail[i].end);
  var columnGaps = [];
  for (var column = 0; column < byColumn.length; column++) {
    for (i = 0; i < byColumn[column].length - 1; i++) {
      columnGaps.push(byColumn[column][i + 1].start - byColumn[column][i].start);
    }
  }
  return 0.5 * sunnyCleanRao(headGaps, 1) + 0.11 * sunnyCleanRao(tailGaps, 1)
    + 1.125 * sunnyCleanRao(columnGaps, 2);
}

function sunnyCleanSwitchPart(times, allCorners, active, weights) {
  if (times.length < 2) return { signature: 0, reference: 0, gaps: 0 };
  var gaps = new Array(times.length - 1);
  for (var i = 0; i < gaps.length; i++) gaps[i] = times[i + 1] - times[i];
  var signature = 0;
  var referenceSum = 0;
  for (i = 0; i < gaps.length; i++) {
    var start = Math.max(0, i - 50);
    var end = Math.min(i + 50, gaps.length - 1);
    var average = 0;
    for (var j = start; j <= end; j++) average += gaps[j];
    average /= end - start + 1;
    if (average <= 0) continue;
    var index = bisectLeft(allCorners, times[i]);
    var ratio = gaps[i] / average;
    signature += Math.sqrt(ratio / gaps.length * weights[index]) * Math.pow(active[index], 0.25);
    referenceSum += ratio * weights[index];
  }
  return { signature: signature, reference: Math.sqrt(referenceSum), gaps: gaps.length };
}

function sunnyCleanSwitches(notes, tails, allCorners, active, weights) {
  var head = sunnyCleanSwitchPart(notes.map(function (note) { return note.start; }), allCorners, active, weights);
  var tail = sunnyCleanSwitchPart(tails.map(function (note) { return note.end; }), allCorners, active, weights);
  var denominator = head.reference * head.gaps + tail.reference * tail.gaps;
  if (!(denominator > 0)) return 0.75;
  return (head.signature * head.gaps + tail.signature * tail.gaps) / denominator / 2 + 0.5;
}

function calculateFromParsedClean(parsed, mod, options) {
  var keys = Number(parsed && parsed[0]);
  if (!Number.isInteger(keys) || keys < 1 || keys > 10) return sunnyCleanZero();
  var notes = sunnyCleanNormalize(parsed, mod);
  if (notes.length < 2) return sunnyCleanZero();
  var od = Number(parsed[5]);
  if (!Number.isFinite(od)) od = 8;
  var x = sunnyCleanHitLeniency(od);
  var byColumn = sunnyCleanGroupColumns(notes, keys);
  var longNotes = notes.filter(function (note) { return note.end >= 0; });
  var duration = Math.max(notes[notes.length - 1].start, longNotes.length ? Math.max.apply(null, longNotes.map(function (note) { return note.end; })) : -1) + 1;
  var corners = sunnyCleanCorners(notes, duration);
  var usage = sunnyCleanKeyUsage(notes, keys, duration, corners.base);
  var activeColumns = sunnyCleanActiveColumns(usage);
  var anchor = sunnyCleanAnchor(notes, keys, duration, corners.base);
  var j = sunnyCleanJ(keys, x, byColumn, corners.base);
  var xBase = sunnyCleanX(keys, x, byColumn, activeColumns, corners.base);
  var bodies = sunnyCleanLnBodies(longNotes, duration);
  var pBase = sunnyCleanP(notes, x, anchor, corners.base, bodies);
  var aBase = sunnyCleanA(keys, activeColumns, j.deltas, corners.unevenness, corners.base);
  var release = sunnyCleanR(longNotes, byColumn, x, corners.base);
  var countBase = sunnyCleanCounts(notes, keys, usage, corners.base);

  var jAll = interpValues(corners.all, corners.base, j.values);
  var xAll = interpValues(corners.all, corners.base, xBase);
  var pAll = interpValues(corners.all, corners.base, pBase);
  var aAll = interpValues(corners.all, corners.unevenness, aBase);
  var rAll = interpValues(corners.all, corners.base, release.values);
  var counts = stepInterp(corners.all, corners.base, countBase.counts);
  var active = stepInterp(corners.all, corners.base, countBase.active);
  var difficulty = new Float64Array(corners.all.length);
  var gaps = new Float64Array(corners.all.length);
  var weights = new Float64Array(corners.all.length);

  for (var i = 0; i < corners.all.length; i++) {
    var cappedJ = Math.min(jAll[i], 8 + 0.85 * jAll[i]);
    var first = Math.pow(aAll[i], 3 / active[i]) * cappedJ;
    var second = Math.pow(aAll[i], 2 / 3) * (0.8 * pAll[i] + rAll[i] * 35 / (counts[i] + 8));
    var strain = Math.pow(0.4 * Math.pow(first, 1.5) + 0.6 * Math.pow(second, 1.5), 2 / 3);
    var twist = Math.pow(aAll[i], 3 / active[i]) * xAll[i] / (xAll[i] + strain + 1);
    difficulty[i] = 2.7 * Math.sqrt(strain) * Math.pow(twist, 1.5) + 0.27 * strain;
  }
  gaps[0] = (corners.all[1] - corners.all[0]) / 2;
  gaps[gaps.length - 1] = (corners.all[gaps.length - 1] - corners.all[gaps.length - 2]) / 2;
  for (i = 1; i < gaps.length - 1; i++) gaps[i] = (corners.all[i + 1] - corners.all[i - 1]) / 2;
  for (i = 0; i < weights.length; i++) weights[i] = counts[i] * gaps[i];

  var order = Array.from({ length: difficulty.length }, function (_, index) { return index; });
  order.sort(function (left, right) { return difficulty[left] - difficulty[right]; });
  var totalWeight = 0;
  for (i = 0; i < weights.length; i++) totalWeight += weights[i];
  if (!(totalWeight > 0)) return sunnyCleanZero();
  var targets = [0.945, 0.935, 0.925, 0.915, 0.845, 0.835, 0.825, 0.815];
  var targetValues = new Array(targets.length);
  var cumulative = 0;
  var targetOrder = targets.map(function (target, index) { return { target: target, index: index }; })
    .sort(function (a, b) { return a.target - b.target; });
  var nextTarget = 0;
  var weightedPower = 0;
  for (i = 0; i < order.length; i++) {
    var sortedIndex = order[i];
    cumulative += weights[sortedIndex];
    weightedPower += Math.pow(difficulty[sortedIndex], 5) * weights[sortedIndex];
    while (nextTarget < targetOrder.length && cumulative / totalWeight >= targetOrder[nextTarget].target) {
      targetValues[targetOrder[nextTarget].index] = difficulty[sortedIndex];
      nextTarget++;
    }
  }
  var percentile93 = 0.25 * (targetValues[0] + targetValues[1] + targetValues[2] + targetValues[3]);
  var percentile83 = 0.25 * (targetValues[4] + targetValues[5] + targetValues[6] + targetValues[7]);
  var mean = Math.pow(weightedPower / totalWeight, 0.2);
  var sr = 0.25 * 0.88 * percentile93 + 0.2 * 0.94 * percentile83 + 0.55 * mean;
  var effectiveNotes = notes.length;
  for (i = 0; i < longNotes.length; i++) effectiveNotes += 0.5 * Math.min(longNotes[i].end - longNotes[i].start, 1000) / 200;
  sr *= effectiveNotes / (effectiveNotes + 60);
  if (sr > 9) sr = 9 + (sr - 9) / 1.2;
  sr *= 0.975;

  var variance = 0;
  for (i = 0; i < difficulty.length; i++) {
    variance += Math.pow(Math.pow(difficulty[i], 8) - Math.pow(mean, 8), 2) * weights[i];
  }
  var weightedVariance = Math.pow(variance / totalWeight, 1 / 8);
  var spikiness = mean > 0 ? Math.sqrt(weightedVariance) / mean : 0;

  var graph = options && options.withGraph === true
    ? {
      times: Array.from(corners.all),
      values: Array.from(difficulty)
    }
    : null;

  return {
    sr: Number.isFinite(sr) ? sr : 0,
    spikiness: Number.isFinite(spikiness) ? spikiness : 0,
    switches: sunnyCleanSwitches(notes, release.tails, corners.all, active, difficulty),
    variety: sunnyCleanVariety(notes, byColumn),
    graph: graph
  };
}

export function calculateFromParsed(parsed, mod, options) {
  return calculateFromParsedClean(parsed, mod || 'NM', options || {});
}

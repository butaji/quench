export function median(values) {
  if (!values.length || values.some((x) => !Number.isFinite(x))) return null;
  const a = [...values].sort((a, b) => a - b), i = a.length >> 1;
  return a.length % 2 ? a[i] : (a[i - 1] + a[i]) / 2;
}

export function random(seed = 17) {
  let state = seed >>> 0;
  return () => { state = (Math.imul(state, 1664525) + 1013904223) >>> 0; return state / 4294967296; };
}

export function ratioInterval(pairs, threshold, samples = 10000) {
  if (pairs.length < 2 || pairs.some(([a, b]) => !Number.isFinite(a) || !Number.isFinite(b) || a <= 0 || b <= 0)) {
    return { verdict: "invalid", reason: "missing positive independent paired measurements" };
  }
  const logs = pairs.map(([a, b]) => Math.log(a / b)), rng = random(104729);
  const distribution = [];
  for (let i = 0; i < samples; i++) {
    let sum = 0;
    for (let j = 0; j < logs.length; j++) sum += logs[Math.floor(rng() * logs.length)];
    distribution.push(Math.exp(sum / logs.length));
  }
  distribution.sort((a, b) => a - b);
  const lower = distribution[Math.floor(samples * 0.05)], upper = distribution[Math.ceil(samples * 0.95) - 1];
  return { pairs: pairs.length, ratio: Math.exp(logs.reduce((a, b) => a + b, 0) / logs.length),
    lower, upper, threshold, method: "paired log-mean percentile bootstrap; one-sided 95% bounds",
    verdict: upper <= threshold ? "pass" : lower > threshold ? "fail" : "inconclusive" };
}

function lifecyclePoints(samples) {
  const byEpoch = new Map();
  for (const sample of samples) {
    if (sample.epoch > 60 && sample.epoch <= 120 && Number.isFinite(sample.rss)) {
      const values = byEpoch.get(sample.epoch) || [];
      values.push(sample.rss); byEpoch.set(sample.epoch, values);
    }
  }
  return [...byEpoch].sort(([a], [b]) => a - b).map(([epoch, values]) => ({ epoch, rss: median(values) }));
}

export function lifecycleVerdict(samples, floor = 8388608, fraction = 0.05) {
  const points = lifecyclePoints(samples);
  if (points.length < 30 || points[0].epoch > 70 || points.at(-1).epoch < 110) {
    return { verdict: "inconclusive", reason: "insufficient late-epoch RSS coverage; increase fixed epochCalls", sampledEpochs: points.length };
  }
  const meanX = points.reduce((s, p) => s + p.epoch, 0) / points.length;
  const meanY = points.reduce((s, p) => s + p.rss, 0) / points.length;
  const denominator = points.reduce((s, p) => s + (p.epoch - meanX) ** 2, 0);
  const slope = points.reduce((s, p) => s + (p.epoch - meanX) * (p.rss - meanY), 0) / denominator;
  const early = points.filter((p) => p.epoch <= 90).map((p) => p.rss);
  const late = points.filter((p) => p.epoch > 90).map((p) => p.rss);
  if (!early.length || !late.length) return { verdict: "inconclusive", reason: "missing lifecycle half" };
  const bound = Math.max(floor, fraction * median(points.map((p) => p.rss)));
  const fittedGrowth = Math.max(0, slope * 59), halfGrowth = Math.max(0, median(late) - median(early));
  return { verdict: fittedGrowth <= bound && halfGrowth <= bound ? "pass" : "fail",
    bound, fittedGrowth, halfGrowth, sampledEpochs: points.length,
    limitation: "Finite sampled RSS plateau evidence, not proof of leak freedom or immediate reclamation." };
}

export function timePerCall(sample) {
  const windows = sample.payload?.windows;
  if (!windows?.length || sample.payload.clock === "date") return null;
  const calls = windows.reduce((s, w) => s + w.calls, 0);
  return windows.reduce((s, w) => s + w.elapsed_ns, 0) / calls;
}

import { median, timePerCall } from "./statistics.mjs";

function durations(row, engine) {
  return (row.timingSamples || []).map((p) => timePerCall(p[engine]));
}

export function contrasts(report, cases) {
  const output = [];
  for (const c of cases) {
    const rows = report.results.filter((r) => r.scenario.experiment === c.id && r.correctness === "pass");
    for (const row of rows) {
      const baseline = rows.find((r) => r.scenario.variant === c.variants[0] && r.scenario.n === row.scenario.n && r.scenario.seed === row.scenario.seed);
      if (!baseline || baseline === row) continue;
      const engines = {};
      for (const engine of ["candidate", "comparator"]) {
        const a = median(durations(row, engine)), b = median(durations(baseline, engine));
        engines[engine] = a !== null && b > 0 ? a / b : null;
      }
      output.push({ experiment: c.id, control: baseline.scenario.id, contrast: row.scenario.id,
        equivalentResults: !!c.equivalent?.some((g) => g.includes(row.scenario.variant) && g.includes(baseline.scenario.variant)),
        relativeTime: engines, confidence: "descriptive contrast; not a causal claim",
        competingExplanations: c.explanations, observationsToRequest: c.observations });
    }
  }
  return output;
}

export function scaling(report) {
  const groups = new Map();
  for (const r of report.results) {
    if (r.correctness !== "pass" || !r.timingSamples?.length) continue;
    const key = `${r.scenario.experiment}/${r.scenario.variant}/${r.scenario.seed}`;
    const points = groups.get(key) || [];
    points.push({ n: r.scenario.n, candidate: median(durations(r, "candidate")), comparator: median(durations(r, "comparator")) });
    groups.set(key, points);
  }
  return [...groups].map(([id, points]) => ({ id, points: points.sort((a, b) => a.n - b.n),
    completeness: new Set(points.map((p) => p.n)).size >= 3 ? "three input sizes" : "more input sizes needed",
    interpretation: "Measured invocation time versus declared input size; do not infer complexity from one point." }));
}

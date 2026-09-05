function traceFrom(stderr) {
  const lines = stderr.split(/\r?\n/).filter((l) => l.startsWith("QUENCH_EXEC_TRACE "));
  if (lines.length !== 1) return { status: "unavailable", reason: "Existing engine did not emit exactly one trace snapshot." };
  try { return { status: "available", snapshot: JSON.parse(lines[0].slice(18)) }; }
  catch { return { status: "invalid", reason: "Malformed engine trace JSON." }; }
}

function numericLeaves(value, prefix = "", result = []) {
  if (typeof value === "number") result.push({ metric: prefix, value });
  else if (value && !Array.isArray(value) && typeof value === "object") {
    for (const [key, child] of Object.entries(value)) numericLeaves(child, prefix ? `${prefix}.${key}` : key, result);
  }
  return result;
}

export function adaptTrace(sample, level, site, build) {
  const parsed = traceFrom(sample.stderr);
  if (parsed.status !== "available") return parsed;
  const snapshot = parsed.snapshot;
  const rows = snapshot.lanes?.l2?.top_compact_sites || [];
  const sites = rows.map((r) => ({ ...r, id: `${build.sha256}:${r.store}:${r.code}:${r.pc}`,
    attribution: "build-qualified engine code/PC; source is an opaque engine identifier, not a verified source span" }));
  const selected = site ? sites.filter((s) => s.id === site || `${s.code}:${s.pc}` === site) : sites;
  const counters = numericLeaves(snapshot);
  const unavailable = level === "events" ? "This engine snapshot has no chronological event capability; no VM changes were made." : null;
  return { status: unavailable ? "unavailable" : "available", reason: unavailable, level,
    capabilities: { counters: true, sites: rows.length > 0, events: false },
    scope: "whole diagnostic process including setup and validation; not timed-phase-only",
    completeness: "Engine top lists are partial. Absent entries are unknown, never zero. Collector drops may be unreported.",
    siteSelection: site ? { requested: site, matched: selected.length, complete: false } : null,
    counters: counters.slice(0, 4096), omittedCounterRows: Math.max(0, counters.length - 4096),
    sites: level === "counters" ? [] : selected.slice(0, 4096),
    rawSnapshot: snapshot,
    metricContract: "Engine-emitted values retain original names/units. Counts are not CPU time; undocumented metrics are observations only." };
}

export function nextExperiment(report, cases) {
  const rows = report.results || [];
  const bad = rows.find((r) => r.correctness === "fail" || r.correctness === "invalid");
  if (bad) return { experiment: bad.scenario.experiment, action: "Resolve the semantic mismatch or incomplete run before interpreting cost.", confidence: "observed" };
  const failed = rows.filter((r) => ["fail", "inconclusive", "invalid"].includes(r.timing?.verdict) || ["fail", "inconclusive", "invalid"].includes(r.memory?.verdict));
  const ids = new Set(failed.map((r) => r.scenario.experiment));
  const candidates = cases.filter((c) => ids.has(c.id)).sort((a, b) => a.id.localeCompare(b.id));
  const chosen = candidates.find((c) => c.requires.every((id) => !ids.has(id))) || candidates[0];
  if (!chosen) return { action: "Run unmeasured sizes, reserved variants, or qualification; this report alone does not prove qualification.", confidence: "unknown" };
  const present = new Set(rows.filter((r) => r.scenario.experiment === chosen.id).map((r) => r.scenario.variant));
  const missing = chosen.variants.filter((v) => !present.has(v));
  const missingPrerequisites = chosen.requires.filter((id) => !rows.some((r) => r.scenario.experiment === id));
  return { experiment: chosen.id, question: chosen.question, confidence: "observed weakness; cause unproven",
    missingContrasts: missing, missingPrerequisites, alternatives: chosen.explanations,
    action: missing.length ? "Run the missing controls/contrasts." : "Compare size sweeps and request attributable counters/sites to distinguish the listed explanations.",
    observationsToRequest: chosen.observations, commands: nextCommands(chosen.id, report) };
}

function nextCommands(id, report) {
  const flags = [["candidate", "--engine"], ["comparator", "--bun"], ["oracle", "--oracle"]]
    .flatMap(([engine, flag]) => report.engines?.[engine]?.path ? [flag, report.engines[engine].path] : []);
  return [
    ["node", "quench-bench/micros/run.mjs", "measure", id, "--size", "all", ...flags],
    ["node", "quench-bench/micros/run.mjs", "diagnose", id, "--instrument", "sites", ...flags]
  ];
}

export function neutralityVerdict(results) {
  if (!results.length) return "invalid";
  if (results.some((r) => r.correctness !== "pass")) return "fail";
  const gates = results.flatMap((r) => [r.timing?.verdict, r.memory?.verdict, ...(r.lifecycle || []).map((x) => x.verdict)]);
  if (gates.includes("fail")) return "fail";
  if (gates.some((v) => !v || v === "invalid")) return "invalid";
  if (gates.includes("inconclusive")) return "inconclusive";
  return "pass";
}

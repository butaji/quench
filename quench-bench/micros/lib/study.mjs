import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawnSync } from "node:child_process";
import { ROOT, hash, editionIdentity } from "./catalog.mjs";
import { binaryIdentity, runProcess } from "./process.mjs";
import { ratioInterval, timePerCall, lifecycleVerdict, random, median } from "./statistics.mjs";
import { nextExperiment, neutralityVerdict, adaptTrace } from "./diagnostics.mjs";
import { contrasts, scaling } from "./contrasts.mjs";

export function newReport(command, catalog, binaries, options) {
  return { schema: 1, command, startedAt: new Date().toISOString(), edition: editionIdentity(catalog),
    host: { platform: process.platform, arch: process.arch, release: os.release(), cpu: os.cpus()[0]?.model, cores: os.cpus().length },
    engines: Object.fromEntries(Object.entries(binaries).map(([k, b]) => [k, binaryIdentity(b)])),
    options, results: [], complete: false, qualification: "not-requested", warnings: [],
    environmentHashes: environmentHashes(),
    sourceTree: { head: spawnSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).stdout.trim(),
      diffHash: hash(spawnSync("git", ["diff", "--binary"], { maxBuffer: 64 * 1024 * 1024 }).stdout || ""),
      status: spawnSync("git", ["status", "--short"], { encoding: "utf8" }).stdout.trim() } };
}

function environmentHashes() {
  const keys = ["NODE_OPTIONS", "BUN_OPTIONS", "QUENCH_ENABLE_AARCH64_STENCILS"];
  return Object.fromEntries(keys.filter((k) => process.env[k] !== undefined).map((k) => [k, hash(process.env[k])]));
}

export function saveReport(report, out, catalog) {
  report.next = nextExperiment(report, catalog.cases);
  report.contrasts = contrasts(report, catalog.cases);
  report.scaling = scaling(report);
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, JSON.stringify(report, null, 2) + "\n");
  const failures = report.results.filter((r) => r.correctness !== "pass" || r.timing?.verdict === "fail" || r.memory?.verdict === "fail");
  const lines = [`# Microbench evidence`, ``, `Command: ${report.command}; complete: ${report.complete}; qualification: ${report.qualification}.`,
    ``, `${report.results.length} scenarios recorded; ${failures.length} scenarios have observed failures.`,
    ``, `## Findings`, ``, ...report.results.map(summaryLine), ``, `## Next experiment`, ``,
    "```json", JSON.stringify(report.next, null, 2), "```", "",
    "Diagnostic internals never affect performance verdicts. Timing differences alone do not prove a root cause.",
    "", "## Contrasts", "", ...report.contrasts.map((c) => `- ${c.contrast} versus ${c.control}: relative time ${JSON.stringify(c.relativeTime)}. Alternatives: ${c.competingExplanations.join(", ")}.`),
    "", "## Scaling", "", ...report.scaling.map((s) => `- ${s.id}: ${s.completeness}; ${JSON.stringify(s.points)}`)];
  fs.writeFileSync(out.replace(/\.json$/, "") + ".md", lines.join("\n") + "\n");
}

function summaryLine(r) {
  return `- ${r.scenario.id}: correctness=${r.correctness}; time ${metricSummary(r.timing)}; RSS ${metricSummary(r.memory)}.`;
}

function metricSummary(metric) { return metric ? `ratio=${metric.ratio?.toFixed(3) || "unavailable"} (${metric.verdict})` : "unmeasured"; }

function opts(options, mode) { return { ...options, mode }; }
function equal(sample, expected) { return sample.valid && expected.valid && sample.payload.result === expected.payload.result; }

export async function correctness(c, scenario, binaries, options) {
  const samples = {};
  for (const [name, binary] of Object.entries(binaries)) samples[name] = await runProcess(binary, c, scenario, opts({ ...options, fixedCalls: 2 }, "verify"));
  const pass = Object.values(samples).every((s) => equal(s, samples.oracle));
  return { correctness: pass ? "pass" : "fail", verification: samples };
}

async function paired(c, scenario, binaries, options, count, mode, expected) {
  const result = [], rng = random(scenario.seed + count);
  const first = rng() < 0.5 ? ["candidate", "comparator"] : ["comparator", "candidate"];
  for (let i = 0; i < count; i++) {
    const order = i % 2 ? [...first].reverse() : first, pair = { order };
    for (const engine of order) pair[engine] = await runProcess(binaries[engine], c, scenario, opts(options, mode));
    pair.correct = equal(pair.candidate, expected) && equal(pair.comparator, expected);
    result.push(pair);
    if (!pair.correct) break;
  }
  return result;
}

export async function measureScenario(c, scenario, binaries, options, verify) {
  const row = { scenario, ...verify, timingSamples: [], memorySamples: [], lifecycle: [] };
  if (verify.correctness !== "pass") return row;
  const expected = verify.verification.oracle;
  row.timingSamples = await paired(c, scenario, binaries, options, options.timingPairs, "throughput", expected);
  row.memorySamples = await paired(c, scenario, binaries, options, options.memoryPairs, "memory", expected);
  if ([...row.timingSamples, ...row.memorySamples].some((p) => !p.correct)) row.correctness = "fail";
  row.timing = ratioInterval(row.timingSamples.map((p) => [timePerCall(p.candidate), timePerCall(p.comparator)]), options.timeRatio, options.bootstrapSamples);
  row.memory = ratioInterval(row.memorySamples.map((p) => [p.candidate.peakRss, p.comparator.peakRss]), options.rssRatio, options.bootstrapSamples);
  if (options.lifecycle && c.memory && row.correctness === "pass") await lifecycle(c, scenario, binaries, options, expected, row);
  return row;
}

async function lifecycle(c, scenario, binaries, options, expected, row) {
  const perCall = median(row.timingSamples.map((p) => timePerCall(p.candidate)));
  const calls = lifecycleCalls(perCall, options);
  const lifecycleOptions = { ...options, epochCalls: calls };
  row.lifecycleCalibration = { epochCalls: calls, timePerCall: perCall, targetEpochMs: options.epochMinMs,
    rule: "Calibrated once from candidate timing; fixed work thereafter across all epochs and replicates." };
  for (let i = 0; i < options.lifecycleRuns; i++) {
    const sample = await runProcess(binaries.candidate, c, scenario, opts(lifecycleOptions, "lifecycle"));
    const result = equal(sample, expected) ? lifecycleVerdict(sample.rssSamples, options.lifecycleFloorBytes, options.lifecycleFraction)
      : { verdict: "invalid", reason: "lifecycle process failed semantic validation" };
    row.lifecycle.push({ ...result, sample });
    if (!equal(sample, expected)) row.correctness = "fail";
  }
}

export function lifecycleCalls(perCall, options) {
  if (!(perCall > 0)) return options.epochCalls;
  return Math.min(options.epochMaxCalls, Math.max(options.epochCalls, Math.ceil(options.epochMinMs * 1000000 / perCall)));
}

export async function diagnoseScenario(c, scenario, binaries, options, verify, report) {
  const row = { scenario, ...verify };
  if (!options.traceEngine) { row.diagnostics = { status: "unavailable", reason: "Supply --trace-engine pointing to an existing instrumented binary; this harness does not modify/build the VM." }; return row; }
  const sample = await runProcess(options.traceEngine, c, scenario, opts({ ...options, env: { ...process.env, QUENCH_EXEC_TRACE: "1" } }, "diagnostic"));
  const identity = binaryIdentity(options.traceEngine);
  row.instrumentedSample = sample;
  row.diagnostics = adaptTrace(sample, options.instrument, options.site, identity);
  row.diagnostics.build = identity;
  row.diagnostics.semanticAgreement = equal(sample, verify.verification.oracle);
  if (!row.diagnostics.semanticAgreement) {
    row.diagnostics.status = "invalid";
    row.diagnostics.reason = "Instrumented execution did not reproduce the oracle result; do not use it to explain the measured workload.";
  }
  row.diagnostics.performanceEvidence = "Not measured here. Use a separate uninstrumented measure report from the same source revision.";
  report.warnings.push(...(!sample.valid ? [`${scenario.id}: instrumented execution failed`] : []));
  return row;
}

export function checkEquivalent(report, c) {
  for (const group of c.equivalent || []) {
    const seen = new Map();
    for (const row of report.results.filter((r) => r.scenario.experiment === c.id && group.includes(r.scenario.variant))) {
      const key = `${row.scenario.size}:${row.scenario.seed}`, value = row.verification?.oracle;
      if (!value?.valid) continue;
      if (seen.has(key) && seen.get(key) !== value.payload.result) {
        row.correctness = "invalid";
        row.reason = "Declared equivalent controls disagree on oracle: corpus defect.";
      }
      seen.set(key, value.payload.result);
    }
  }
}

export function finalize(report, catalog, total, qualification) {
  report.complete = report.results.length === total;
  for (const c of catalog.cases) checkEquivalent(report, c);
  const changed = Object.values(report.engines).some((b) => hash(fs.readFileSync(b.path)) !== b.sha256);
  if (changed) report.warnings.push("Engine binary changed during run; results invalid for qualification.");
  if (editionIdentity(catalog).digest !== report.edition.digest) report.warnings.push("Corpus changed during run; results invalid for qualification.");
  if (qualification) report.qualification = !report.complete || report.warnings.length ? "invalid" : neutralityVerdict(report.results);
  report.finishedAt = new Date().toISOString();
}

export function assertEdition(catalog, requested) {
  const filename = path.join(ROOT, "editions", `${requested}.json`);
  const locked = JSON.parse(fs.readFileSync(filename, "utf8"));
  const actual = editionIdentity(catalog);
  if (requested !== catalog.config.edition || locked.digest !== actual.digest) throw new Error("Edition differs from current corpus/protocol. Create a new edition; do not rewrite history.");
}

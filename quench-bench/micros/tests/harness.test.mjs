import test from "node:test";
import assert from "node:assert/strict";
import { loadCatalog, scenarios, validateGraph, editionIdentity } from "../lib/catalog.mjs";
import { ratioInterval, lifecycleVerdict } from "../lib/statistics.mjs";
import { parseOutput, parseRss, runProcess, payloadSource, childEnvironment } from "../lib/process.mjs";
import { adaptTrace, neutralityVerdict, nextExperiment } from "../lib/diagnostics.mjs";
import { checkEquivalent, finalize, lifecycleCalls } from "../lib/study.mjs";
import { contrasts, scaling } from "../lib/contrasts.mjs";

const catalog = loadCatalog();
const scenario = { id: "fixture/control/small/17", variant: "control", n: 64, seed: 17 };
const options = { mode: "verify", fixedCalls: 2, timeoutMs: 3000 };
function fixture(body, extra = "") {
  return { source: `registerMicro({setup:function(){return {i:0};},variants:{control:function(s){${body}}}${extra}});` };
}

test("real executable coverage, prerequisites, and unlimited catalog", () => {
  assert.ok(catalog.cases.length >= 24);
  assert.ok(catalog.cases.reduce((n, c) => n + c.variants.length, 0) >= 124);
  assert.ok(catalog.legacy.length >= 100);
  validateGraph(Array.from({ length: 1200 }, (_, i) => ({ id: `case-${i}`, requires: [] })));
  assert.throws(() => validateGraph([{ id: "a", requires: ["b"] }]), /missing/);
  assert.throws(() => validateGraph([{ id: "a", requires: ["a"] }]), /cycle/);
  assert.throws(() => validateGraph([{ id: "a", requires: [] }, { id: "a", requires: [] }]), /duplicate/);
});

test("reserved scenarios preserve deterministic separation and every variant", () => {
  const dev = scenarios(catalog.cases, catalog.config), reserved = scenarios(catalog.cases, catalog.config, true);
  assert.equal(reserved.length, catalog.cases.reduce((n, c) => n + c.variants.length, 0) * Object.keys(catalog.config.sizes).length * catalog.config.seeds.qualification.length);
  assert.equal(new Set(reserved.map((s) => s.id)).size, reserved.length);
  assert.ok(reserved.every((r) => r.sourceForm === "wrapped" && !dev.some((d) => d.seed === r.seed)));
  assert.equal(editionIdentity(catalog).digest, editionIdentity(catalog).digest);
});

test("paired statistics distinguish pass, loss, uncertainty and missing measurements", () => {
  assert.equal(ratioInterval([[8, 10], [8, 10]], 0.95).verdict, "pass");
  assert.equal(ratioInterval([[11, 10], [11, 10]], 0.95).verdict, "fail");
  assert.equal(ratioInterval([[8, 10], [12, 10]], 0.95).verdict, "inconclusive");
  assert.equal(ratioInterval([[null, 10], [8, 10]], 0.95).verdict, "invalid");
  assert.equal(ratioInterval([[0, 10], [8, 10]], 0.95).verdict, "invalid");
});

test("lifecycle distinguishes plateau, deliberate growth, and insufficient sampling", () => {
  const stable = Array.from({ length: 120 }, (_, i) => ({ epoch: i + 1, rss: 32 * 1024 * 1024 }));
  const growing = stable.map((s, i) => ({ ...s, rss: s.rss + i * 1024 * 1024 }));
  assert.equal(lifecycleVerdict(stable).verdict, "pass");
  assert.equal(lifecycleVerdict(growing).verdict, "fail");
  assert.equal(lifecycleVerdict(stable.slice(0, 10)).verdict, "inconclusive");
});

test("RSS and physical footprint never share a metric", () => {
  assert.equal(parseRss("123 maximum resident set size", "darwin"), 123);
  assert.equal(parseRss("123 peak memory footprint", "darwin"), null);
  assert.equal(parseRss("Maximum resident set size (kbytes): 123", "linux"), 123 * 1024);
});

test("missing results, malformed output, and unfinished async fail", () => {
  assert.throws(() => parseOutput(""), /completed result/);
  assert.throws(() => parseOutput("MICRO_RESULT {}"), /malformed/);
  assert.throws(() => parseOutput("MICRO_ERROR deliberate"), /deliberate/);
  assert.throws(() => parseOutput("MICRO_RESULT {}\nMICRO_RESULT {}"), /exactly one/);
});

test("instrumentation is optional, partial, and never fabricates event capability", () => {
  assert.equal(adaptTrace({ stderr: "" }, "sites", null, {}).status, "unavailable");
  const sample = { stderr: 'QUENCH_EXEC_TRACE {"lanes":{"l2":{"top_compact_sites":[{"store":"x","code":2,"pc":3,"count":7}]}}}' };
  const sites = adaptTrace(sample, "sites", "2:4", { sha256: "build" });
  assert.equal(sites.siteSelection.matched, 0);
  assert.equal(sites.siteSelection.complete, false);
  assert.match(sites.completeness, /unknown, never zero/);
  assert.equal(adaptTrace(sample, "events", null, {}).status, "unavailable");
});

test("different internal architectures cannot change outcome verdicts", () => {
  const base = { correctness: "pass", timing: { verdict: "pass" }, memory: { verdict: "pass" }, lifecycle: [] };
  assert.equal(neutralityVerdict([{ ...base, diagnostics: { jit: false } }]), "pass");
  assert.equal(neutralityVerdict([{ ...base, diagnostics: { movingGC: true } }]), "pass");
  assert.equal(neutralityVerdict([{ ...base, diagnostics: null }]), "pass");
  assert.equal(neutralityVerdict([{ ...base, timing: null }]), "invalid");
});

test("next requests evidence instead of claiming a root cause", () => {
  const report = { results: [{ scenario: { experiment: "calls", variant: "direct" }, correctness: "pass", timing: { verdict: "fail" } }] };
  const next = nextExperiment(report, catalog.cases);
  assert.equal(next.experiment, "calls");
  assert.ok(next.missingContrasts.includes("inline"));
  assert.match(next.confidence, /unproven/);
});

test("equivalent contrast disagreements invalidate the corpus result", () => {
  const rows = ["inline", "direct"].map((variant, i) => ({ scenario: { experiment: "calls", variant, size: "small", seed: 17 }, correctness: "pass", verification: { oracle: { valid: true, payload: { result: String(i) } } } }));
  checkEquivalent({ results: rows }, catalog.cases.find((c) => c.id === "calls"));
  assert.equal(rows[1].correctness, "invalid");
});

test("qualification cannot pass an incomplete selection", () => {
  const report = { results: [], engines: {}, warnings: [], edition: editionIdentity(catalog) };
  finalize(report, catalog, 10, true);
  assert.equal(report.complete, false);
  assert.equal(report.qualification, "invalid");
});

test("scaling and contrasts retain observations without prescribing internals", () => {
  const rows = ["inline", "direct"].map((variant, i) => ({ scenario: { experiment: "calls", variant, size: "small", n: 64, seed: 17 }, correctness: "pass",
    timingSamples: Array.from({ length: 2 }, () => ({ candidate: { payload: { clock: "hrtime", windows: [{ calls: 10, elapsed_ns: 100 * (i + 1) }] } }, comparator: { payload: { clock: "hrtime", windows: [{ calls: 10, elapsed_ns: 50 }] } } })) }));
  assert.equal(contrasts({ results: rows }, catalog.cases)[0].relativeTime.candidate, 2);
  assert.match(scaling({ results: rows })[0].completeness, /more/);
});

test("real subprocess validates distinguished JS values", async () => {
  const sample = await runProcess(process.execPath, fixture("return [undefined, -0, NaN, Infinity, 7n, '\\ud800'];"), scenario, options);
  assert.equal(sample.valid, true, sample.reason);
  assert.match(sample.payload.result, /undefined/);
  assert.match(sample.payload.result, /-0/);
  assert.match(sample.payload.result, /NaN/);
  assert.match(sample.payload.result, /bigint/);
});

test("observable assertions and changing results fail", async () => {
  const a = await runProcess(process.execPath, fixture("return ++s.i;"), scenario, options);
  const b = await runProcess(process.execPath, fixture("return 1;", ",check:function(){throw new Error('effect lost');}"), scenario, options);
  assert.equal(a.valid, false);
  assert.equal(b.valid, false);
  assert.match(b.reason, /effect lost/);
});

test("timeout kills an uncompleted workload", async () => {
  const sample = await runProcess(process.execPath, fixture("while(true){}"), scenario, { ...options, timeoutMs: 150 });
  assert.equal(sample.valid, false);
  assert.equal(sample.timedOut, true);
});

test("async work must complete before measurement result", async () => {
  const pass = await runProcess(process.execPath, fixture("return Promise.resolve(7);", ",async:true"), scenario, options);
  const unfinished = await runProcess(process.execPath, fixture("return new Promise(function(){});", ",async:true"), scenario, options);
  assert.equal(pass.valid, true, pass.reason);
  assert.equal(unfinished.valid, false);
});

test("plain payload contains no engine-specific branching", () => {
  const source = payloadSource(catalog.cases[0], scenario, options);
  assert.doesNotMatch(source, /Bun|QUENCH_EXEC_TRACE|quench-node/);
});

test("lifecycle calibration is bounded and fixed before epochs", () => {
  const p = catalog.config.protocol;
  assert.equal(lifecycleCalls(1000000, p), 250);
  assert.equal(lifecycleCalls(1, p), p.epochMaxCalls);
  assert.equal(lifecycleCalls(null, p), p.epochCalls);
});

test("scored child cannot inherit trace enablement", () => {
  const env = { PATH: process.env.PATH, QUENCH_EXEC_TRACE: "1", QUENCH_LOOP_TRACE: "1" };
  assert.equal(childEnvironment({ mode: "throughput", env }).QUENCH_EXEC_TRACE, undefined);
  assert.equal(childEnvironment({ mode: "memory", env }).QUENCH_LOOP_TRACE, undefined);
  assert.equal(childEnvironment({ mode: "diagnostic", env }).QUENCH_EXEC_TRACE, "1");
});

test("legacy wrapper preserves directive prologues", async () => {
  const c = { legacy: true, source: '"use strict"; const result = (function(){return this === undefined;})();' };
  const sample = await runProcess(process.execPath, c, { ...scenario, variant: "original" }, options);
  assert.equal(sample.valid, true, sample.reason);
  assert.equal(sample.payload.result, '["boolean",true]');
});

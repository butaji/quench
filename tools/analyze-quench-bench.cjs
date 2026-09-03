#!/usr/bin/env node
"use strict";

const cp = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");
const { formatViolations, violations } = require("./lib/profile-contracts.cjs");

const root = path.resolve(__dirname, "..");
const suite = path.join(root, "quench-bench/js-engine-benchmark/v8-v7");
const args = process.argv.slice(2);
const fixtureArg = args[0];
if (!fixtureArg || fixtureArg.startsWith("--")) usage("missing fixture");
const option = (name, fallback) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : fallback;
};
const fixture = fixtureArg.endsWith(".js") ? fixtureArg : `${fixtureArg}.js`;
const quench = path.resolve(root, option("--quench", "target/bench-throughput/quench-node"));
const traceQuench = path.resolve(root, option("--trace-quench", "target-exec-trace/bench-throughput/quench-node"));
const sampleQuench = path.resolve(root, option("--sample-quench", quench));
const timeoutMs = Number(option("--timeout-ms", "120000"));
const output = option("--out", null);
const baselinePath = option("--baseline", null);
const sampleSeconds = Number(option("--sample-seconds", "0"));
const skipTrace = args.includes("--skip-trace");
const dumpStderr = args.includes("--dump-stderr");
const contractPath = option("--assert-profile", null);
if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) usage("invalid timeout");

const runner = `
let __ok = true;
const __print = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { __print(name + ": " + result); },
  NotifyError(name, error) { __ok = false; __print(name + ": " + error); },
  NotifyScore(score) { if (__ok) __print("Score: " + score); },
});
`;

function usage(message) {
  console.error(`${message}\nusage: analyze-quench-bench.cjs FIXTURE [--quench PATH] [--trace-quench PATH] [--sample-quench PATH] [--timeout-ms N] [--sample-seconds N] [--baseline JSON] [--out JSON] [--assert-profile JSON]`);
  process.exit(2);
}

function materialize() {
  const fixturePath = path.join(suite, fixture);
  if (!fs.existsSync(fixturePath)) usage(`unknown fixture: ${fixture}`);
  const destination = path.join(
    "/tmp",
    `quench-analysis-${process.pid}-${Date.now()}-${fixture}`
  );
  fs.writeFileSync(destination, [
    fs.readFileSync(path.join(suite, "base.js"), "utf8"),
    fs.readFileSync(fixturePath, "utf8"),
    runner,
  ].join("\n"));
  return destination;
}

function metric(stderr, label) {
  for (const line of stderr.split(/\n/)) {
    const normalized = line.trim();
    const suffix = normalized.match(new RegExp(`^([0-9]+)\\s+${label}$`, "i"));
    const colon = normalized.match(new RegExp(`^${label}:\\s*([0-9]+)$`, "i"));
    const value = suffix?.[1] || colon?.[1];
    if (value) return Number(value);
  }
  return null;
}

function ranked(trace) {
  const rows = [];
  for (const [name, count] of Object.entries(trace.compact || {}))
    rows.push({ layer: "compact", name, count });
  for (const [name, count] of Object.entries(trace.slow || {}))
    rows.push({ layer: "slow", name, count });
  return rows.sort((left, right) => right.count - left.count);
}

function delta(current, baseline) {
  if (!baseline) return null;
  const ratio = (before, after) => before && after ? before / after : null;
  return {
    wall_speedup: ratio(baseline.host.wall_ms, current.host.wall_ms),
    instruction_reduction: ratio(baseline.host.instructions, current.host.instructions),
    rss_reduction: ratio(baseline.host.peak_rss_bytes, current.host.peak_rss_bytes),
    guest_reduction: ratio(baseline.vm.guest_total, current.vm.guest_total),
    score_speedup: ratio(current.score, baseline.score),
    lane_ratios: Object.fromEntries(Object.entries(current.ratios).map(([name, value]) => [
      name, ratio(baseline.ratios?.[name], value),
    ])),
  };
}

function laneRatios(vm) {
  const lanes = vm.lanes || {};
  const l2 = lanes.l2?.handlers || 0;
  const l3 = lanes.l3?.handlers || 0;
  const handlers = l2 + l3;
  const calls = Object.values(vm.call_targets || {}).reduce((sum, count) => sum + count, 0);
  return {
    value_decode_per_handler: handlers ? (lanes.l0?.value_decode || 0) / handlers : null,
    counted_hits_per_l2_handler: l2 ? (lanes.l1?.counted?.hits || 0) / l2 : null,
    crypto_hit_rate: lanes.l1?.crypto?.direct_iterations
      ? lanes.l1.crypto.hits / lanes.l1.crypto.direct_iterations : null,
    environments_per_call: calls ? (vm.heap_lifecycle?.environment?.allocated || 0) / calls : null,
    l3_handler_share: handlers ? l3 / handlers : null,
    counted_hits_per_loop_iteration: vm.loop_iteration
      ? (lanes.l1?.counted?.hits || 0) / vm.loop_iteration : null,
  };
}

function execute(binary, script, trace) {
  const env = trace ? ["env", "QUENCH_EXEC_TRACE=1"] : [];
  const start = process.hrtime.bigint();
  const result = cp.spawnSync("timeout", [
    "--signal=TERM", "--kill-after=1", String(timeoutMs / 1000),
    "/usr/bin/time", "-l", ...env, binary, script,
  ], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return { result, wallMs: Number(process.hrtime.bigint() - start) / 1e6 };
}

async function sample(script) {
  if (!(sampleSeconds > 0)) return null;
  const probe = cp.spawn(sampleQuench, [script], { stdio: "ignore" });
  await new Promise((resolve) => setTimeout(resolve, 1000));
  const destination = `/tmp/quench-analysis-${fixture}.sample`;
  const result = cp.spawnSync("sample", [String(probe.pid), String(sampleSeconds), "-file", destination], { encoding: "utf8" });
  probe.kill("SIGKILL");
  if (result.status !== 0) return { available: false, binary: sampleQuench, path: null };
  const text = fs.readFileSync(destination, "utf8");
  const self = text.match(/Sort by top of stack, same collapsed[^\n]*\n([\s\S]*?)\nBinary Images:/)?.[1] || "";
  const rows = [...self.matchAll(/^\s+(.+?)\s+\(in quench-node\)\s+(\d+)$/gm)]
    .map((match) => ({ symbol: match[1], samples: Number(match[2]) }))
    .sort((left, right) => right.samples - left.samples);
  const total = rows.reduce((sum, row) => sum + row.samples, 0);
  return {
    available: true,
    binary: sampleQuench,
    path: destination,
    top_self: rows.slice(0, 32).map((row) => ({
      ...row,
      share_ppm: total ? Math.round(row.samples * 1_000_000 / total) : 0,
    })),
  };
}

async function main() {
  const script = materialize();
  try {
  const scored = execute(quench, script, false);
  if (dumpStderr && scored.result.stderr) process.stderr.write(scored.result.stderr);
  const traced = skipTrace ? null : execute(traceQuench, script, true);
  const traceLine = traced && (traced.result.stderr || "").split(/\n/).find((line) => line.startsWith("QUENCH_EXEC_TRACE "));
  if (traced && !traceLine) throw new Error(`missing VM trace; status=${traced.result.status}\n${traced.result.stderr || ""}`);
  const vm = traceLine ? JSON.parse(traceLine.slice("QUENCH_EXEC_TRACE ".length)) : {};
  const stdout = scored.result.stdout || "";
  const report = {
    lanes: vm.lanes,
    heap_lifecycle: vm.heap_lifecycle,
    function_call_shapes: (vm.function_call_shapes || []).slice(0, 8),
    loop_shapes: (vm.loop_shapes || []).slice(0, 8),
    schema: 2,
    fixture,
    binary: quench,
    trace_binary: traceQuench,
    valid: scored.result.status === 0 && (!traced || traced.result.status === 0) && /(^|\n)Score:\s*[0-9.]+/m.test(stdout),
    score: Number(stdout.match(/(^|\n)Score:\s*([0-9.]+)/m)?.[2]) || null,
    host: {
      wall_ms: scored.wallMs,
      peak_rss_bytes: metric(scored.result.stderr || "", "maximum resident set size"),
      instructions: metric(scored.result.stderr || "", "instructions retired"),
      cycles: metric(scored.result.stderr || "", "cycles elapsed"),
      page_faults: metric(scored.result.stderr || "", "page faults"),
      page_reclaims: metric(scored.result.stderr || "", "page reclaims"),
      involuntary_context_switches: metric(scored.result.stderr || "", "involuntary context switches"),
    },
    vm,
    ranked_origins: ranked(vm).slice(0, 30),
    sample: await sample(script),
  };
  report.ratios = laneRatios(vm);
  const baseline = baselinePath ? JSON.parse(fs.readFileSync(baselinePath, "utf8")) : null;
  report.delta = delta(report, baseline);
  const contracts = contractPath ? JSON.parse(fs.readFileSync(contractPath, "utf8")) : null;
  const failures = contracts ? violations(report, contracts) : [];
  report.profile_contract = contracts ? { passed: failures.length === 0, failures } : null;
  const json = `${JSON.stringify(report, null, 2)}\n`;
  if (output) fs.writeFileSync(output, json);
  process.stdout.write(json);
  if (failures.length) console.error(formatViolations(fixture.replace(/\.js$/, ""), failures));
  if (!report.valid || failures.length) process.exitCode = 1;
  } finally {
    // A timeout, missing trace, or parse failure must not leave a source
    // snapshot that a later analysis could accidentally consume.
    fs.rmSync(script, { force: true });
  }
}

main().catch((error) => {
  console.error(error.stack || error);
  process.exit(1);
});

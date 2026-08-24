#!/usr/bin/env node
"use strict";

const cp = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

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
const timeoutMs = Number(option("--timeout-ms", "180000"));
const output = option("--out", null);
const baselinePath = option("--baseline", null);
const sampleSeconds = Number(option("--sample-seconds", "0"));
if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) usage("invalid timeout");

const runner = `
let __ok = true;
const __print = typeof console !== "undefined" ? console.log : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { __print(name + ": " + result); },
  NotifyError(name, error) { __ok = false; __print(name + ": " + error); },
  NotifyScore(score) { if (__ok) __print("Score: " + score); },
});
`;

function usage(message) {
  console.error(`${message}\nusage: analyze-quench-bench.cjs FIXTURE [--quench PATH] [--timeout-ms N] [--sample-seconds N] [--baseline JSON] [--out JSON]`);
  process.exit(2);
}

function materialize() {
  const fixturePath = path.join(suite, fixture);
  if (!fs.existsSync(fixturePath)) usage(`unknown fixture: ${fixture}`);
  const destination = path.join("/tmp", `quench-analysis-${fixture}`);
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
  };
}

function explain(report) {
  const vm = report.vm;
  const events = vm.events || {};
  const guest = vm.guest_total || 1;
  const loopIterations = events.loop_iteration || 0;
  const slowGateways = vm.compact?.Slow || 0;
  const directCompact = vm.compact_total - slowGateways;
  return {
    slow_dispatch_share: vm.slow_total / guest,
    host_instructions_per_guest: report.host.instructions === null
      ? null : report.host.instructions / guest,
    guest_ops_per_loop_iteration: loopIterations ? guest / loopIterations : null,
    value_decodes_per_loop_iteration: loopIterations
      ? (events.value_decode || 0) / loopIterations : null,
    fragment_entries_per_loop_iteration: loopIterations
      ? (events.fragment_entry || 0) / loopIterations : null,
    diagnosis: [
      vm.slow_total > directCompact
        ? "cold Op dispatch dominates compact retirement"
        : "compact retirement dominates cold Op dispatch",
      (events.fragment_entry || 0) > (events.loop_entry || 0) * 2
        ? "loop control repeatedly enters nested fragments"
        : "nested fragment entry is not the leading loop multiplier",
      (events.value_decode || 0) > guest / 2
        ? "Value materialization is frequent relative to guest retirement"
        : "Value materialization is below half of guest retirement",
    ],
  };
}

async function sample(script) {
  if (!(sampleSeconds > 0)) return null;
  const probe = cp.spawn(quench, [script], { stdio: "ignore" });
  await new Promise((resolve) => setTimeout(resolve, 1000));
  const destination = `/tmp/quench-analysis-${fixture}.sample`;
  const result = cp.spawnSync("sample", [String(probe.pid), String(sampleSeconds), "-file", destination], { encoding: "utf8" });
  probe.kill("SIGKILL");
  return { available: result.status === 0, path: result.status === 0 ? destination : null };
}

async function main() {
  const script = materialize();
  const start = process.hrtime.bigint();
  const result = cp.spawnSync("timeout", [
    "--signal=TERM", "--kill-after=1", String(timeoutMs / 1000),
    "/usr/bin/time", "-l", "env", "QUENCH_EXEC_TRACE=1", quench, script,
  ], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  const wallMs = Number(process.hrtime.bigint() - start) / 1e6;
  const traceLine = (result.stderr || "").split(/\n/).find((line) => line.startsWith("QUENCH_EXEC_TRACE "));
  if (!traceLine) throw new Error(`missing VM trace; status=${result.status}\n${result.stderr || ""}`);
  const vm = JSON.parse(traceLine.slice("QUENCH_EXEC_TRACE ".length));
  const report = {
    schema: 1,
    fixture,
    binary: quench,
    valid: result.status === 0 && /(^|\n)Score:\s*[0-9.]+/m.test(result.stdout || ""),
    score: Number((result.stdout || "").match(/(^|\n)Score:\s*([0-9.]+)/m)?.[2]) || null,
    host: {
      wall_ms: wallMs,
      peak_rss_bytes: metric(result.stderr || "", "maximum resident set size"),
      instructions: metric(result.stderr || "", "instructions retired"),
      cycles: metric(result.stderr || "", "cycles elapsed"),
      page_faults: metric(result.stderr || "", "page faults"),
      page_reclaims: metric(result.stderr || "", "page reclaims"),
      involuntary_context_switches: metric(result.stderr || "", "involuntary context switches"),
    },
    vm,
    ranked_origins: ranked(vm).slice(0, 30),
    sample: await sample(script),
  };
  report.analysis = explain(report);
  const baseline = baselinePath ? JSON.parse(fs.readFileSync(baselinePath, "utf8")) : null;
  report.delta = delta(report, baseline);
  const json = `${JSON.stringify(report, null, 2)}\n`;
  if (output) fs.writeFileSync(output, json);
  process.stdout.write(json);
  if (!report.valid) process.exitCode = 1;
}

main().catch((error) => {
  console.error(error.stack || error);
  process.exit(1);
});

#!/usr/bin/env node
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync
} from "node:fs";
import { join, resolve } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const suite = resolve(import.meta.dirname, "js-engine-benchmark", "v8-v7");
const runtime = resolve(root, "target", "bench-throughput", "quench-node");
const fixtures = [
  "richards.js",
  "deltablue.js",
  "crypto.js",
  "raytrace.js",
  "earley-boyer.js",
  "regexp.js",
  "splay.js",
  "navier-stokes.js"
];
const runner = `
let __quenchBenchSucceeded = true;
const __quenchBenchPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { __quenchBenchPrint(name + ": " + result); },
  NotifyError(name, error) { __quenchBenchSucceeded = false; __quenchBenchPrint(name + ": " + error); },
  NotifyScore(score) {
    if (__quenchBenchSucceeded) {
      __quenchBenchPrint("----");
      __quenchBenchPrint("Score: " + score);
    }
  },
});
`;
const DEFAULT_TIMEOUT_MS = 120_000;

function usage() {
  console.log(
    "usage: node quench-bench/run-quench-runtime.mjs [--node PATH] [--quench PATH] [--runs N] [--only suite1,suite2] [--timeout-ms N] [--out FILE]"
  );
}
function arg(name, fallback) {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}
if (process.argv.includes("--help")) {
  usage();
  process.exit(0);
}
const node = arg("--node", process.execPath);
const quench = arg("--quench", arg("--binary", runtime));
const runs = Number(arg("--runs", "3"));
if (!Number.isInteger(runs) || runs <= 0) {
  throw new Error("--runs must be a positive integer");
}
const timeoutMs = Number(arg("--timeout-ms", String(DEFAULT_TIMEOUT_MS)));
if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
  throw new Error("--timeout-ms must be a positive number");
}
const output = arg("--out", null);
const selected = arg(
  "--only",
  fixtures.map((name) => name.slice(0, -3)).join(",")
)
  .split(",")
  .filter(Boolean)
  .map((name) => (name.endsWith(".js") ? name : `${name}.js`));

function measure(command, script) {
  const start = process.hrtime.bigint();
  const result = spawnSync(command, [script], {
    encoding: "utf8",
    timeout: timeoutMs,
    killSignal: "SIGKILL"
  });
  const wallNs = Number(process.hrtime.bigint() - start);
  const stdout = result.stdout || "";
  const score = Number(stdout.match(/^Score:\s*([\d.]+)$/m)?.[1]);
  return {
    command,
    script,
    status: result.status,
    timedOut: result.error?.code === "ETIMEDOUT",
    wallNs,
    score: Number.isFinite(score) ? score : null,
    stdout,
    stderr: result.stderr || ""
  };
}

if (!existsSync(suite)) throw new Error(`benchmark suite not found: ${suite}`);
const results = {};
for (const fixture of selected) {
  if (!fixtures.includes(fixture))
    throw new Error(`unknown benchmark fixture: ${fixture}`);
  const script = join(suite, fixture);
  const base = readFileSync(join(suite, "base.js"), "utf8");
  const source = base + "\n" + readFileSync(script, "utf8") + "\n" + runner;
  // Give every invocation a fresh scratch directory. A killed runner can
  // leave its files behind, but a later run can never consume those bytes.
  const scratch = mkdtempSync(join(tmpdir(), "quench-bench-"));
  const temp = join(scratch, fixture);
  try {
    writeFileSync(temp, source);
  } catch (error) {
    rmSync(scratch, { recursive: true, force: true });
    throw error;
  }
  const nodeRuns = Array.from({ length: runs }, () => measure(node, temp));
  const quenchRuns = Array.from({ length: runs }, () => measure(quench, temp));
  const outputEqual = nodeRuns.at(-1).stdout === quenchRuns.at(-1).stdout;
  // Benchmark scores are engine-dependent; retain outputEqual as evidence but
  // do not reject an otherwise successful scored run because timings differ.
  const valid = [...nodeRuns, ...quenchRuns].every(
    (sample) => sample.status === 0 && !sample.timedOut && sample.score !== null
  );
  if (!valid) process.exitCode = 1;
  const summary = {
    fixture,
    valid,
    node: nodeRuns.map(({ status, timedOut, wallNs, score }) => ({
      status,
      timedOut,
      wallNs,
      score
    })),
    quench: quenchRuns.map(({ status, timedOut, wallNs, score }) => ({
      status,
      timedOut,
      wallNs,
      score
    })),
    outputEqual,
    diagnostics: valid ? undefined : {
      node: nodeRuns.map(({ stdout, stderr }) => ({ stdout, stderr })),
      quench: quenchRuns.map(({ stdout, stderr }) => ({ stdout, stderr }))
    }
  };
  console.log(JSON.stringify(summary));
  results[fixture.slice(0, -3)] = {
    node: summary.node,
    quench: summary.quench,
    valid,
    output_equal: summary.outputEqual
  };
  rmSync(scratch, { recursive: true, force: true });
}
if (output) writeFileSync(output, JSON.stringify({ results }, null, 2) + "\n");

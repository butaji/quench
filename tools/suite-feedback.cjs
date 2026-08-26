#!/usr/bin/env node
"use strict";

const cp = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const benchmark = path.join(root, "quench-bench/run-quench-runtime.mjs");
const scoreBinary = path.join(root, "target/bench-throughput/quench-node");
const traceBinary = path.join(
  root,
  "target-exec-trace/bench-throughput/quench-node"
);
const timeoutRunner = path.join(root, "tools/run-with-timeout.cjs");
const outputPath = "/tmp/quench-suite-row.json";
const floors = Object.freeze({
  richards: 52430,
  deltablue: 69195,
  crypto: 72214,
  raytrace: 185359,
  "earley-boyer": 87457,
  regexp: 22983,
  splay: 50113,
  "navier-stokes": 34337
});
const expectedKernels = Object.freeze({
  richards: ["L|S|C"],
  deltablue: [
    "plan_execute_direct",
    "plan_execute_loop",
    "counted_method_affine",
    "counted_method_copy_property"
  ],
  crypto: ["crypto_integer_multiply", "crypto_kernel"],
  raytrace: ["raytrace_render", "raytrace_pixel"],
  "earley-boyer": ["pair_word_walk", "pair_walk"],
  regexp: ["regexp_exact_global_exec"],
  splay: ["bump_tiny_object", "splay_rotate"],
  "navier-stokes": [
    "counted_packed_f64",
    "navier_numeric_kernel",
    "counted_for"
  ]
});
const microContracts = Object.freeze({
  deltablue: ["affine-plan.want.json", "plan-execute-loop.want.json"],
  richards: ["linked-shape-call.want.json"],
  crypto: ["limb-fill.want.json"],
  raytrace: ["vec3-dot-add.want.json"],
  "earley-boyer": ["pair-car-cdr.want.json"],
  regexp: ["exec-discard.want.json"],
  splay: ["cons-bump.want.json"],
  "navier-stokes": ["packed-add.want.json"]
});

function fail(message) {
  console.error(message);
  process.exit(2);
}

function run(command, args, options = {}) {
  const timeout = options.timeout || 600000;
  const spawnOptions = { ...options };
  delete spawnOptions.timeout;
  const result = cp.spawnSync(
    process.execPath,
    [timeoutRunner, String(timeout), command, ...args],
    {
      cwd: root,
      encoding: "utf8",
      timeout: timeout + 10000,
      maxBuffer: 64 * 1024 * 1024,
      ...spawnOptions
    }
  );
  if (result.status !== 0) {
    const detail = [result.stdout, result.stderr].filter(Boolean).join("\n");
    throw new Error(`${command} failed (${result.status})\n${detail}`);
  }
  return result;
}

function build() {
  run("cargo", ["build", "--profile", "bench-throughput", "-p", "quench-node"]);
  run(
    "cargo",
    [
      "build",
      "--profile",
      "bench-throughput",
      "-p",
      "quench-node",
      "--features",
      "execution-trace"
    ],
    {
      env: {
        ...process.env,
        CARGO_TARGET_DIR: path.join(root, "target-exec-trace")
      }
    }
  );
}

function median(values) {
  const ordered = [...values].sort((left, right) => left - right);
  return ordered[Math.floor(ordered.length / 2)] ?? null;
}

function scoreSuites(suites) {
  const result = run(
    process.execPath,
    [
      benchmark,
      "--runs",
      "3",
      "--only",
      suites.join(","),
      "--quench",
      scoreBinary
    ],
    {
      timeout: Math.max(300000, suites.length * 6 * 130000)
    }
  );
  const summaries = new Map();
  for (const line of result.stdout.split(/\n/)) {
    if (!line.startsWith("{")) continue;
    const summary = JSON.parse(line);
    summaries.set(summary.fixture.replace(/\.js$/, ""), summary);
  }
  return summaries;
}

function traceSuite(suite) {
  const script = path.join("/tmp", `quench-bench-${suite}.js`);
  const result = run(traceBinary, [script], {
    timeout: 180000,
    env: { ...process.env, QUENCH_EXEC_TRACE: "1" }
  });
  const line = (result.stderr || "")
    .split(/\n/)
    .find((candidate) => candidate.startsWith("QUENCH_EXEC_TRACE "));
  if (result.status !== 0 || !line) {
    throw new Error(
      `trace failed for ${suite} (${result.status})\n${result.stdout || ""}\n${result.stderr || ""}`
    );
  }
  return JSON.parse(line.slice("QUENCH_EXEC_TRACE ".length));
}

function rankedCount(rows, key) {
  return rows?.find((row) => row.op === key)?.count || 0;
}

function kernelMeasurement(trace, suite) {
  const kernels = trace.lanes?.l1?.kernels || [];
  const kernel = expectedKernels[suite]
    .map((id) => kernels.find((candidate) => candidate.id === id))
    .find(Boolean);
  const loops = trace.events?.loop_iteration || 0;
  return {
    kernel_id: kernel?.id || "NONE",
    hits_per_loop: kernel ? kernel.hits / Math.max(loops, 1) : 0,
    deopt: kernel?.deopts || 0
  };
}

function rowFor(suite, summary, trace) {
  const quenchScores =
    summary?.quench?.map((sample) => sample.score).filter(Number.isFinite) ||
    [];
  const nodeScores =
    summary?.node?.map((sample) => sample.score).filter(Number.isFinite) || [];
  const valid = Boolean(
    summary?.valid && quenchScores.length === 3 && nodeScores.length === 3
  );
  const score = valid ? median(quenchScores) : null;
  const node = valid ? median(nodeScores) : null;
  const kernel = kernelMeasurement(trace, suite);
  return {
    suite,
    score_m3: score,
    node_m3: node,
    floor: floors[suite],
    pct_floor: score === null ? null : score / floors[suite],
    ...kernel,
    calln: rankedCount(trace.lanes?.l0?.owned_word_read_by_op, "CallN"),
    getn: rankedCount(trace.lanes?.l0?.owned_word_read_by_op, "GetN"),
    env_alloc: trace.heap_lifecycle?.environment?.allocated || 0,
    valid
  };
}

function printRow(row) {
  const fields = [
    "suite",
    "score_m3",
    "node_m3",
    "floor",
    "pct_floor",
    "kernel_id",
    "hits_per_loop",
    "calln",
    "getn",
    "env_alloc",
    "deopt",
    "valid"
  ];
  console.log(fields.join("\t"));
  console.log(
    fields
      .map((field) => {
        const value = row[field];
        return typeof value === "number" && !Number.isInteger(value)
          ? value.toFixed(6)
          : String(value);
      })
      .join("\t")
  );
}

function hasMicroContract(suite) {
  return (microContracts[suite] || []).some((file) =>
    fs.existsSync(path.join(root, "tests/lanes", file))
  );
}

function main() {
  const requested = process.argv[2];
  if (!requested) fail("usage: node tools/suite-feedback.cjs SUITE|--all");
  const suites =
    requested === "--all"
      ? Object.keys(floors)
      : [requested.replace(/\.js$/, "")];
  for (const suite of suites)
    if (!(suite in floors)) fail(`unknown suite: ${suite}`);
  build();

  const regressionSuites =
    requested === "--all"
      ? suites
      : [...new Set([...suites, "richards", "navier-stokes"])];
  const summaries = scoreSuites(regressionSuites);
  const rows = suites.map((suite) =>
    rowFor(suite, summaries.get(suite), traceSuite(suite))
  );
  const report = { schema: 1, generated_at: new Date().toISOString(), rows };
  fs.writeFileSync(outputPath, `${JSON.stringify(report, null, 2)}\n`);
  rows.forEach(printRow);
  console.log(`json\t${outputPath}`);

  let invalid = false;
  for (const row of rows) {
    if (!row.valid) invalid = true;
    if (hasMicroContract(row.suite) && row.hits_per_loop < 0.05) invalid = true;
  }
  const richards = summaries.get("richards");
  const navier = summaries.get("navier-stokes");
  if (
    !richards?.valid ||
    median(richards.quench.map((sample) => sample.score)) < 40000
  )
    invalid = true;
  if (
    !navier?.valid ||
    median(navier.quench.map((sample) => sample.score)) < 34000
  )
    invalid = true;
  if (invalid) process.exitCode = 1;
}

try {
  main();
} catch (error) {
  console.error(error.stack || error);
  process.exit(1);
}

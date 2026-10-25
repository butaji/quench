#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(root, "../..");
const workloadName = process.argv[2];
const phase = process.argv[3] ?? "no-jit";
const runs = Number(process.argv[4] ?? 1);
const outputPath = process.argv[5];
const workload = path.resolve(root, "workloads", `${workloadName}.cjs`);
const linuxRssSampleIntervalMs = Number(process.env.QUENCH_NPM_PERF_RSS_SAMPLE_MS ?? 5);

if (!workloadName || !["no-jit", "jit"].includes(phase) ||
  !Number.isSafeInteger(runs) || runs < 1 || !fs.existsSync(workload) ||
  !Number.isFinite(linuxRssSampleIntervalMs) || linuxRssSampleIntervalMs <= 0) {
  console.error(
    "usage: node run-workload.mjs WORKLOAD no-jit|jit RUNS [REPORT.json]",
  );
  process.exit(2);
}

const binaries = {
  quench: process.env.QUENCH_NODE_BIN ?? path.join(repo, "target/production/quench-node"),
  quenchControl: process.env.QUENCH_CONTROL_BIN,
  node: process.env.NODE_BIN ?? process.execPath,
  bun: process.env.BUN_BIN ?? path.join(repo, "work/bun/bun"),
};

function median(values) {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

function rssCommand(binary, args) {
  if (process.platform === "darwin") {
    return { command: "/usr/bin/time", args: ["-l", binary, ...args] };
  }
  if (process.platform === "linux") return null;
  throw new Error(`maximum RSS measurement is not implemented on ${process.platform}`);
}

function parseRss(stderr) {
  const mac = stderr.match(/([\d,]+)\s+maximum resident set size/);
  if (mac) return Number(mac[1].replaceAll(",", ""));
  throw new Error("timing tool did not report maximum resident set size");
}

function engineSpecs() {
  if (phase === "no-jit") {
    const specs = [
      { name: "quench-interpreter", binary: binaries.quench, args: [workload], env: {} },
    ];
    if (binaries.quenchControl) {
      const controlEnv = {};
      if (process.env.QUENCH_CONTROL_GC_MAX_YOUNG) {
        controlEnv.QUENCH_GC_MAX_YOUNG = process.env.QUENCH_CONTROL_GC_MAX_YOUNG;
      }
      specs.push({ name: "quench-control", binary: binaries.quenchControl, args: [workload], env: controlEnv });
    }
    if (process.env.QUENCH_NPM_PERF_SKIP_NODE_NO_JIT !== "1") {
      specs.push({ name: "node-jitless", binary: binaries.node, args: ["--jitless", workload], env: {} });
    }
    specs.push({ name: "bun-no-jit", binary: binaries.bun, args: [workload], env: { BUN_JSC_useJIT: "0" } });
    return specs;
  }
  const specs = [
    { name: "quench-jit", binary: binaries.quench, args: ["--jit", workload], env: {} },
  ];
  if (binaries.quenchControl) {
    specs.push({ name: "quench-control-jit", binary: binaries.quenchControl, args: ["--jit", workload], env: {} });
  }
  specs.push(
    { name: "node-jit", binary: binaries.node, args: [workload], env: {} },
    { name: "bun-jit", binary: binaries.bun, args: [workload], env: {} },
  );
  return specs;
}

function linuxProcessTreeSnapshot(rootPid) {
  let total = 0;
  let rootHighWaterRssBytes = 0;
  const pending = [rootPid];
  const visited = new Set();
  const descendants = [];
  while (pending.length) {
    const pid = pending.pop();
    if (visited.has(pid)) continue;
    visited.add(pid);
    try {
      const status = fs.readFileSync(`/proc/${pid}/status`, "utf8");
      const resident = Number(status.match(/^VmRSS:\s+(\d+)\s+kB$/m)?.[1] ?? 0) * 1024;
      const highWater = Number(status.match(/^VmHWM:\s+(\d+)\s+kB$/m)?.[1] ?? 0) * 1024;
      total += resident;
      if (pid === rootPid) rootHighWaterRssBytes = highWater;
      if (pid !== rootPid) descendants.push({ pid, highWaterRssBytes: highWater });
      const children = fs.readFileSync(`/proc/${pid}/task/${pid}/children`, "utf8")
        .trim()
        .split(/\s+/)
        .filter(Boolean)
        .map(Number);
      pending.push(...children);
    } catch {
      // A process can exit between reading its status and its children list.
    }
  }
  return { rssBytes: total, rootHighWaterRssBytes, descendants };
}

async function runLinuxChild(spec, env) {
  const child = spawn(spec.binary, spec.args, { cwd: root, env, stdio: ["ignore", "pipe", "pipe"] });
  let stdout = "";
  let stderr = "";
  let maxRssBytes = 0;
  let maxProcessRssBytes = 0;
  const descendantSamples = new Map();
  child.stdout.setEncoding("utf8").on("data", (chunk) => { stdout += chunk; });
  child.stderr.setEncoding("utf8").on("data", (chunk) => { stderr += chunk; });
  const sample = () => {
    const tree = linuxProcessTreeSnapshot(child.pid);
    // VmHWM is maintained by the kernel; the short interval gives us a chance
    // to read it before a fast workload exits. Descendant RSS remains sampled.
    maxProcessRssBytes = Math.max(maxProcessRssBytes, tree.rootHighWaterRssBytes ?? 0);
    maxRssBytes = Math.max(maxRssBytes, tree.rssBytes);
    for (const { pid, highWaterRssBytes } of tree.descendants) {
      const observed = descendantSamples.get(pid) ?? { samples: 0, highWaterRssBytes: 0 };
      observed.samples += 1;
      observed.highWaterRssBytes = Math.max(observed.highWaterRssBytes, highWaterRssBytes);
      descendantSamples.set(pid, observed);
    }
  };
  const sampler = setInterval(sample, linuxRssSampleIntervalMs);
  sample();
  const timeout = setTimeout(() => child.kill("SIGKILL"), 300_000);
  const result = await new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("close", (status, signal) => resolve({ status, signal }));
  });
  sample();
  clearInterval(sampler);
  clearTimeout(timeout);
  return {
    ...result,
    stdout,
    stderr,
    maxRssBytes,
    maxProcessRssBytes,
    descendantProcessCount: descendantSamples.size,
    maxObservedDescendantSamples: Math.max(
      0,
      ...[...descendantSamples.values()].map(({ samples }) => samples),
    ),
    observedProcessTreeHwmSumBytes: maxProcessRssBytes + [...descendantSamples.values()]
      .reduce((total, { highWaterRssBytes }) => total + highWaterRssBytes, 0),
  };
}

async function runOne(spec, round) {
  const env = { ...process.env, ...spec.env };
  env.NODE_ENV = "production";
  delete env.NODE_OPTIONS;
  if (spec.name !== "bun-no-jit") delete env.BUN_JSC_useJIT;
  if (spec.name !== "bun-no-jit") delete env.BUN_JSC_dumpOptions;
  if (spec.name === "bun-no-jit") env.BUN_JSC_dumpOptions = "1";
  const start = process.hrtime.bigint();
  const timed = rssCommand(spec.binary, spec.args);
  const result = timed
    ? spawnSync(timed.command, timed.args, {
      cwd: root,
      encoding: "utf8",
      env,
      timeout: 300_000,
      maxBuffer: 16 * 1024 * 1024,
    })
    : await runLinuxChild(spec, env);
  const wallTimeMs = Number(process.hrtime.bigint() - start) / 1e6;
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${spec.name} exited ${result.status ?? result.signal}:\n${result.stderr}`);
  }
  if (spec.name === "bun-no-jit" && !result.stderr.includes("useJIT=false")) {
    throw new Error("Bun JIT-off option dump did not confirm useJIT=false");
  }
  if (spec.name === "node-jitless") {
    const proof = spawnSync(spec.binary, ["--jitless", "-e", "console.log(process.execArgv.includes('--jitless'))"], { encoding: "utf8", env });
    if (proof.status !== 0 || proof.stdout.trim() !== "true") {
      throw new Error("Node --jitless argv proof failed");
    }
  }
  const records = result.stdout.trim().split(/\r?\n/);
  const measurement = JSON.parse(records.at(-1));
  if (measurement.workload !== workloadName ||
    !Number.isFinite(measurement.operationsPerSecond) ||
    !Number.isFinite(measurement.elapsedMs)) {
    throw new Error(`${spec.name} returned an invalid benchmark record`);
  }
  return {
    engine: spec.name,
    round,
    measurement,
    maxRssBytes: timed ? parseRss(result.stderr) : result.maxRssBytes,
    ...(result.maxProcessRssBytes === undefined ? {} : {
      maxProcessRssBytes: result.maxProcessRssBytes,
    }),
    ...(result.descendantProcessCount === undefined ? {} : {
      descendantProcessCount: result.descendantProcessCount,
      maxObservedDescendantSamples: result.maxObservedDescendantSamples,
      observedProcessTreeHwmSumBytes: result.observedProcessTreeHwmSumBytes,
    }),
    wallTimeMs,
    stderr: result.stderr.replace(/__QUENCH_MAX_RSS_KB=\d+\n?/g, "").trim(),
    command: [spec.binary, ...spec.args],
    effectiveEnvironment: {
      ...spec.env,
      NODE_ENV: "production",
      QUENCH_BENCH_ROUNDS: env.QUENCH_BENCH_ROUNDS ?? "5000",
      QUENCH_BENCH_WARMUPS: env.QUENCH_BENCH_WARMUPS ?? "250",
      ...Object.fromEntries([
        "QUENCH_MEMORY_POLICY",
        "QUENCH_GC_YOUNG",
        "QUENCH_GC_YOUNG_NUM",
        "QUENCH_GC_YOUNG_DEN",
        "QUENCH_GC_MIN_YOUNG",
        "QUENCH_GC_MAX_YOUNG",
        "QUENCH_GC_OLD_GROWTH",
        "QUENCH_GC_OLD_GROWTH_NUM",
        "QUENCH_GC_OLD_GROWTH_DEN",
        "QUENCH_GC_MIN_OLD_GROWTH",
        "QUENCH_GC_ALLOCATION_CAP",
        "QUENCH_GC_ALLOCATION_CAP_NUM",
        "QUENCH_GC_ALLOCATION_CAP_DEN",
        "QUENCH_GC_MIN_ALLOCATION_CAP",
        "QUENCH_NPM_PERF_SKIP_NODE_NO_JIT",
        "QUENCH_NPM_PERF_RSS_SAMPLE_MS",
      ].flatMap((name) => env[name] === undefined ? [] : [[name, env[name]]])),
    },
  };
}

const specs = engineSpecs();
const samples = [];
let expected;
for (let round = 0; round < runs; round += 1) {
  const order = [...specs];
  const rotation = round % order.length;
  order.push(...order.splice(0, rotation));
  for (const spec of order) {
    const sample = await runOne(spec, round + 1);
    const { rounds: _rounds, elapsedMs: _elapsed, operationsPerSecond: _rate, ...observable } = sample.measurement;
    if (expected && JSON.stringify(observable) !== JSON.stringify(expected)) {
      throw new Error(`${sample.engine} output differed from earlier engines`);
    }
    expected ??= observable;
    samples.push(sample);
  }
}

const summaries = Object.fromEntries(specs.map(({ name }) => {
  const rows = samples.filter((sample) => sample.engine === name);
  return [name, {
    medianOperationsPerSecond: median(rows.map(({ measurement }) => measurement.operationsPerSecond)),
    medianElapsedMs: median(rows.map(({ measurement }) => measurement.elapsedMs)),
    medianMaximumRssBytes: median(rows.map(({ maxRssBytes }) => maxRssBytes)),
    ...(rows.some(({ maxProcessRssBytes }) => maxProcessRssBytes !== undefined) ? {
      medianMaximumProcessRssBytes: median(
        rows.map(({ maxProcessRssBytes }) => maxProcessRssBytes),
      ),
    } : {}),
    ...(rows.some(({ descendantProcessCount }) => descendantProcessCount !== undefined) ? {
      medianObservedDescendantProcessCount: median(
        rows.map(({ descendantProcessCount }) => descendantProcessCount),
      ),
      maximumObservedDescendantSamples: Math.max(
        0,
        ...rows.map(({ maxObservedDescendantSamples }) => maxObservedDescendantSamples),
      ),
      medianObservedProcessTreeHwmSumBytes: median(
        rows.map(({ observedProcessTreeHwmSumBytes }) => observedProcessTreeHwmSumBytes),
      ),
    } : {}),
  }];
}));
const source = fs.readFileSync(workload);
const lock = fs.readFileSync(path.join(root, "package-lock.json"));
const report = {
  schema: 1,
  workload: workloadName,
  phase,
  runs,
  qualificationReady: false,
  ...(phase === "no-jit" && process.env.QUENCH_NPM_PERF_SKIP_NODE_NO_JIT === "1" ? {
    omittedEngines: {
      "node-jitless": process.env.QUENCH_NODE_NO_JIT_SKIP_REASON ?? "Skipped by QUENCH_NPM_PERF_SKIP_NODE_NO_JIT.",
    },
  } : {}),
  host: { platform: process.platform, arch: process.arch, cpus: os.cpus().length },
  rssMeasurement: process.platform === "linux"
    ? `kernel VmHWM for the engine process sampled every ${linuxRssSampleIntervalMs} ms plus descendant RSS sampling and a sum of VmHWM for observed descendant PIDs (may miss short-lived children and counts shared pages per process; not qualification-ready)`
    : "maximum RSS from /usr/bin/time -l",
  inputs: {
    workloadSha256: crypto.createHash("sha256").update(source).digest("hex"),
    packageLockSha256: crypto.createHash("sha256").update(lock).digest("hex"),
    quenchBinarySha256: crypto.createHash("sha256").update(fs.readFileSync(binaries.quench)).digest("hex"),
    ...(binaries.quenchControl ? {
      controlQuenchBinary: binaries.quenchControl,
      controlQuenchBinarySha256: crypto.createHash("sha256").update(fs.readFileSync(binaries.quenchControl)).digest("hex"),
    } : {}),
    nodeBinary: process.execPath,
    bunBinary: binaries.bun,
  },
  nodeVersion: process.version,
  bunVersion: spawnSync(binaries.bun, ["--version"], { encoding: "utf8" }).stdout.trim(),
  summary: summaries,
  output: expected,
  samples,
};

const json = `${JSON.stringify(report, null, 2)}\n`;
if (outputPath) {
  const destination = path.resolve(outputPath);
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.writeFileSync(destination, json, { flag: "wx" });
} else {
  process.stdout.write(json);
}

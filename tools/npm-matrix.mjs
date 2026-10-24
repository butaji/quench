#!/usr/bin/env node
// Ten-package npm benchmark matrix.
//
// Runs every selected workload/phase through tests/npm-perf/run-workload.mjs,
// which executes Quench, Node and Bun in interleaved rounds and verifies that
// all engines produced the same observable record. This runner aggregates the
// per-workload reports, evaluates the 1.20x throughput / 0.80x max-RSS gate
// and writes a new evidence directory. It never overwrites an existing one.

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// Sizes follow the saved dc4fe3b1d cohort: standard rows use 5,000 operations,
// Next route generation / ws / Pino use 50,000 and Zod JIT uses 100,000 so the
// fastest engines do not finish inside the timer floor.
const WORKLOADS = [
  { id: "react-ssr", package: "React / react-dom", rounds: 5000, warmups: 250 },
  { id: "next-route-generate", package: "Next", rounds: 50000, warmups: 250 },
  {
    id: "vite-transform",
    package: "Vite",
    rounds: 5000,
    warmups: 250,
    nodeNoJitUnavailable:
      "Node --jitless removes the WebAssembly global Vite/Oxc requires.",
  },
  { id: "esbuild-transform", package: "esbuild", rounds: 5000, warmups: 250 },
  { id: "sharp-resize-concurrent", package: "sharp", rounds: 1000, warmups: 100 },
  { id: "fastify-json-http", package: "Fastify HTTP", rounds: 5000, warmups: 250 },
  { id: "fastify-json-inject", package: "Fastify inject", rounds: 5000, warmups: 250 },
  {
    id: "undici-pool",
    package: "Undici",
    rounds: 5000,
    warmups: 250,
    nodeNoJitUnavailable:
      "Node --jitless removes the WebAssembly global Undici/llhttp requires.",
  },
  { id: "ws-echo", package: "ws", rounds: 50000, warmups: 50 },
  { id: "pino-json-logger", package: "Pino JSON", rounds: 50000, warmups: 250 },
  {
    id: "zod-parse",
    package: "Zod",
    rounds: { "no-jit": 5000, jit: 100000 },
    warmups: 250,
  },
];

const PHASES = ["no-jit", "jit"];
const GATE = { speed: 1.2, rss: 0.8, minSampleMs: 200 };
const ENGINE_PREFIX = { "no-jit": "quench-interpreter", jit: "quench-jit" };
const PEER_PREFIX = { "no-jit": "bun-no-jit", jit: "bun-jit" };
const NODE_PREFIX = { "no-jit": "node-jitless", jit: "node-jit" };

function usage() {
  process.stdout.write(`usage: node tools/npm-matrix.mjs [options]

Options:
  --rounds N          interleaved rounds per workload/phase (default 11)
  --screen            shorthand for --rounds 3 (can reject, never accept)
  --workload ID       repeatable workload id (default: all)
  --phase MODE        repeatable: no-jit or jit (default: both)
  --out DIR           new evidence directory (default evidence/npm-10-m4-<date>)
  --quench PATH       quench-node binary (default target/production/quench-node)
  --node PATH         Node binary (default the running node)
  --bun PATH          Bun binary (default work/bun/bun, then bun on PATH)
  --no-gate           write evidence but exit 0 even when a gate is missed
  --dry-run           print the jobs without running them
  --help              this text

Workloads: ${WORKLOADS.map((workload) => workload.id).join(", ")}
`);
}

function fail(message) {
  process.stderr.write(`npm-matrix: ${message}\n`);
  process.exit(2);
}

function parseArgs(argv) {
  const options = {
    rounds: 11,
    workloads: new Set(),
    phases: new Set(),
    out: undefined,
    quench: undefined,
    node: undefined,
    bun: undefined,
    gate: true,
    dryRun: false,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    const value = () => {
      const next = argv[++index];
      if (next === undefined) fail(`${argument} needs a value`);
      return next;
    };
    switch (argument) {
      case "--rounds":
        options.rounds = Number(value());
        break;
      case "--screen":
        options.rounds = 3;
        break;
      case "--workload":
        options.workloads.add(value());
        break;
      case "--phase":
        options.phases.add(value());
        break;
      case "--out":
        options.out = value();
        break;
      case "--quench":
        options.quench = value();
        break;
      case "--node":
        options.node = value();
        break;
      case "--bun":
        options.bun = value();
        break;
      case "--no-gate":
        options.gate = false;
        break;
      case "--dry-run":
        options.dryRun = true;
        break;
      case "--help":
        usage();
        process.exit(0);
        break;
      default:
        fail(`unknown option ${argument}`);
    }
  }
  if (!Number.isSafeInteger(options.rounds) || options.rounds < 1) {
    fail("--rounds must be a positive integer");
  }
  for (const id of options.workloads) {
    if (!WORKLOADS.some((workload) => workload.id === id)) {
      fail(`unknown workload ${id}`);
    }
  }
  for (const phase of options.phases) {
    if (!PHASES.includes(phase)) fail(`unknown phase ${phase}`);
  }
  return options;
}

function resolveExecutable(explicit, candidates) {
  if (explicit) {
    return explicit.includes("/") ? path.resolve(explicit) : explicit;
  }
  for (const candidate of candidates) {
    if (candidate.includes("/")) {
      if (fs.existsSync(candidate)) return candidate;
      continue;
    }
    const probe = spawnSync("which", [candidate], { encoding: "utf8" });
    if (probe.status === 0) return probe.stdout.trim();
  }
  return candidates.at(-1);
}

function sha256(file) {
  if (!file || !fs.existsSync(file)) return undefined;
  return crypto.createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

function commandOutput(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  return result.status === 0 ? result.stdout.trim() : undefined;
}

function hostIdentity() {
  const identity = {
    platform: process.platform,
    arch: process.arch,
    cpus: os.cpus().length,
    totalMemoryBytes: os.totalmem(),
  };
  if (process.platform === "darwin") {
    identity.productVersion = commandOutput("sw_vers", ["-productVersion"]);
    identity.buildVersion = commandOutput("sw_vers", ["-buildVersion"]);
  }
  identity.hostname = os.hostname();
  return identity;
}

function gitIdentity() {
  const revision = commandOutput("git", ["rev-parse", "HEAD"]);
  const branch = commandOutput("git", ["rev-parse", "--abbrev-ref", "HEAD"]);
  const status = commandOutput("git", ["status", "--porcelain"]);
  return {
    revision,
    shortRevision: revision?.slice(0, 9),
    branch,
    dirty: Boolean(status),
    dirtyPaths: status ? status.split("\n").slice(0, 20) : [],
  };
}

function resolveRounds(workload, phase) {
  return typeof workload.rounds === "number"
    ? workload.rounds
    : workload.rounds[phase];
}

function jobList(options) {
  const selectedWorkloads = options.workloads.size === 0
    ? WORKLOADS
    : WORKLOADS.filter((workload) => options.workloads.has(workload.id));
  const selectedPhases = options.phases.size === 0
    ? PHASES
    : PHASES.filter((phase) => options.phases.has(phase));
  return selectedWorkloads.flatMap((workload) =>
    selectedPhases.map((phase) => ({
      workload,
      phase,
      rounds: resolveRounds(workload, phase),
      warmups: workload.warmups,
    }))
  );
}

function runJob(job, context) {
  const { workload, phase, rounds, warmups } = job;
  const reportPath = path.join(
    context.outDir,
    `${workload.id}-${phase}-${context.git.shortRevision}-${rounds}.json`,
  );
  const env = {
    ...process.env,
    NODE_ENV: "production",
    QUENCH_NODE_BIN: context.quench,
    NODE_BIN: context.node,
    BUN_BIN: context.bun,
    QUENCH_BENCH_ROUNDS: String(rounds),
    QUENCH_BENCH_WARMUPS: String(warmups),
  };
  delete env.QUENCH_CONTROL_BIN;
  delete env.QUENCH_NPM_PERF_SKIP_NODE_NO_JIT;
  if (phase === "no-jit" && workload.nodeNoJitUnavailable) {
    env.QUENCH_NPM_PERF_SKIP_NODE_NO_JIT = "1";
    env.QUENCH_NODE_NO_JIT_SKIP_REASON = workload.nodeNoJitUnavailable;
  }
  const started = process.hrtime.bigint();
  const result = spawnSync(
    process.execPath,
    [
      path.join(root, "tests/npm-perf/run-workload.mjs"),
      workload.id,
      phase,
      String(context.rounds),
      reportPath,
    ],
    { cwd: root, env, encoding: "utf8", timeout: 6 * 60 * 60 * 1000 },
  );
  const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
  if (result.error) {
    return { job, reportPath, elapsedMs, error: String(result.error) };
  }
  if (result.status !== 0) {
    return {
      job,
      reportPath,
      elapsedMs,
      error: (result.stderr || result.stdout || `exit ${result.status}`).trim(),
    };
  }
  return {
    job,
    reportPath,
    elapsedMs,
    report: JSON.parse(fs.readFileSync(reportPath, "utf8")),
  };
}

function engineSummary(report, prefix) {
  return report.summary[prefix];
}

function evaluate(result) {
  const { job, report } = result;
  const quench = engineSummary(report, ENGINE_PREFIX[job.phase]);
  const bun = engineSummary(report, PEER_PREFIX[job.phase]);
  const node = engineSummary(report, NODE_PREFIX[job.phase]);
  const peers = [node, bun].filter(Boolean);
  if (!quench || peers.length === 0) {
    return { ...result, error: "report is missing the Quench or a peer summary" };
  }
  const fasterPeer = peers.reduce((best, peer) =>
    peer.medianOperationsPerSecond > best.medianOperationsPerSecond ? peer : best
  );
  const lowerRssPeer = peers.reduce((best, peer) =>
    peer.medianMaximumRssBytes < best.medianMaximumRssBytes ? peer : best
  );
  const speedRatio = quench.medianOperationsPerSecond /
    fasterPeer.medianOperationsPerSecond;
  const rssRatio = quench.medianMaximumRssBytes /
    lowerRssPeer.medianMaximumRssBytes;
  const quenchElapsedMs = quench.medianElapsedMs;
  const fastestPeerElapsedMs = Math.min(
    ...peers.map((peer) => peer.medianElapsedMs),
  );
  const row = {
    workload: job.workload.id,
    package: job.workload.package,
    phase: job.phase,
    rounds: job.rounds,
    warmups: job.warmups,
    quench: {
      operationsPerSecond: quench.medianOperationsPerSecond,
      maximumRssBytes: quench.medianMaximumRssBytes,
      elapsedMs: quenchElapsedMs,
    },
    peers: {
      node: node
        ? {
          operationsPerSecond: node.medianOperationsPerSecond,
          maximumRssBytes: node.medianMaximumRssBytes,
        }
        : null,
      bun: {
        operationsPerSecond: bun.medianOperationsPerSecond,
        maximumRssBytes: bun.medianMaximumRssBytes,
      },
    },
    unavailable: report.omittedEngines ?? undefined,
    speedRatio,
    rssRatio,
    speedNeeded: GATE.speed / speedRatio,
    rssReductionNeeded: rssRatio / GATE.rss,
    shortSample: Math.min(quenchElapsedMs, fastestPeerElapsedMs) < GATE.minSampleMs,
    shortestElapsedMs: Math.min(quenchElapsedMs, fastestPeerElapsedMs),
    gate: {
      throughput: speedRatio >= GATE.speed,
      rss: rssRatio <= GATE.rss,
    },
    pass: speedRatio >= GATE.speed && rssRatio <= GATE.rss,
    report: path.basename(result.reportPath),
  };
  return { ...result, row };
}

function formatNumber(value) {
  return Math.round(value).toLocaleString("en-US");
}

function formatMiB(bytes) {
  return (bytes / (1024 * 1024)).toFixed(2);
}

function formatRatio(value) {
  return `${value.toFixed(2)}×`;
}

function markdown(rows, failures, context) {
  const lines = [];
  lines.push("# Ten-package npm benchmark matrix (M4 macOS)");
  lines.push("");
  lines.push(`Generated: ${context.generatedAt}`);
  lines.push("");
  lines.push("Gate: Quench must reach at least `1.20 ×` the faster peer");
  lines.push("throughput and at most `0.80 ×` the lower peer maximum RSS,");
  lines.push("separately for each workload and phase. Maximum RSS is the");
  lines.push("`/usr/bin/time -l` process maximum on this macOS host.");
  lines.push("");
  lines.push(`- Host: ${context.host.platform}/${context.host.arch}, ` +
    `${context.host.cpus} CPUs, ${context.host.hostname}` +
    (context.host.productVersion
      ? `, macOS ${context.host.productVersion} (${context.host.buildVersion})`
      : ""));
  lines.push(`- Revision: \`${context.git.revision}\` on \`${context.git.branch}\`` +
    `${context.git.dirty ? " (dirty tree)" : ""}`);
  lines.push(`- Quench: \`${context.quench}\` SHA-256 \`${context.hashes.quench}\``);
  lines.push(`- Node: \`${context.node}\` ${context.versions.node}`);
  lines.push(`- Bun: \`${context.bun}\` ${context.versions.bun}`);
  lines.push(`- Rounds: ${context.rounds} (interleaved, engine order rotated)`);
  lines.push(`- Command: \`${context.command}\``);
  lines.push("");
  lines.push("Qualification status: this directory records a host-local gate");
  lines.push("verdict, not a qualification. The per-workload reports keep");
  lines.push("`qualificationReady: false`.");
  lines.push("");
  lines.push(
    "| Package | Phase | Quench ops/s | Faster peer | Speed | Quench RSS MiB | Lower peer | RSS | Gate |",
  );
  lines.push("|---|---|---:|---:|---:|---:|---:|---:|---|");
  for (const row of rows) {
    const peerOps = Math.max(
      ...Object.values(row.peers).filter(Boolean).map((peer) => peer.operationsPerSecond),
    );
    const peerRss = Math.min(
      ...Object.values(row.peers).filter(Boolean).map((peer) => peer.maximumRssBytes),
    );
    const verdict = row.pass
      ? "**PASS**"
      : `MISS (speed ${formatRatio(row.speedNeeded)} needed, ` +
        `RSS ${formatRatio(row.rssReductionNeeded)} lower)`;
    lines.push(
      `| ${row.package} | ${row.phase} | ${formatNumber(row.quench.operationsPerSecond)} | ` +
      `${formatNumber(peerOps)} | ${formatRatio(row.speedRatio)} | ` +
      `${formatMiB(row.quench.maximumRssBytes)} | ${formatMiB(peerRss)} | ` +
      `${formatRatio(row.rssRatio)} | ${verdict} |`,
    );
  }
  lines.push("");
  const unavailable = rows.filter((row) => row.unavailable);
  if (unavailable.length > 0) {
    lines.push("Node no-JIT omissions:");
    lines.push("");
    for (const row of unavailable) {
      lines.push(`- ${row.package} ${row.phase}: ` +
        `${row.unavailable["node-jitless"] ?? "unavailable"}`);
    }
    lines.push("");
  }
  const short = rows.filter((row) => row.shortSample);
  if (short.length > 0) {
    lines.push("Samples shorter than the 200 ms timer-floor target (diagnostic):");
    lines.push("");
    for (const row of short) {
      lines.push(`- ${row.package} ${row.phase}: shortest engine ` +
        `${row.shortestElapsedMs.toFixed(0)} ms; extend the workload before ` +
        "treating this row as qualification evidence");
    }
    lines.push("");
  }
  if (failures.length > 0) {
    lines.push("Failed jobs:");
    lines.push("");
    for (const failure of failures) {
      lines.push(`- ${failure.job.workload.id} ${failure.job.phase}: ` +
        failure.error.split("\n")[0]);
    }
    lines.push("");
  }
  const passed = rows.filter((row) => row.pass).length;
  lines.push(`Gate: ${passed}/${rows.length} workload/phase rows passed.`);
  lines.push("");
  lines.push("Run with `node tools/npm-matrix.mjs` to reproduce; each report here");
  lines.push("contains all per-round samples, hashes and output checksums.");
  lines.push("");
  return `${lines.join("\n")}\n`;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  const quench = resolveExecutable(options.quench, [
    path.join(root, "target/production/quench-node"),
  ]);
  const node = resolveExecutable(options.node, [process.execPath]);
  const bun = resolveExecutable(options.bun, [
    path.join(root, "work/bun/bun"),
    "bun",
  ]);
  const git = gitIdentity();
  const jobs = jobList(options);
  if (jobs.length === 0) fail("no workloads selected");
  if (options.dryRun) {
    for (const job of jobs) {
      process.stdout.write(
        `${job.workload.id} ${job.phase} rounds=${job.rounds} warmups=${job.warmups}\n`,
      );
    }
    return;
  }
  if (!fs.existsSync(quench)) {
    fail(`Quench binary not found at ${quench}; run tools/pgo-build.sh production`);
  }
  if (bun.includes("/") && !fs.existsSync(bun)) {
    fail(`Bun binary not found at ${bun}`);
  }
  const stamp = `${new Date().toISOString().slice(0, 10)}T` +
    new Date().toISOString().slice(11, 16).replace(":", "");
  const outDir = path.resolve(
    options.out ?? path.join(root, "evidence", `npm-10-m4-${stamp}`),
  );
  if (fs.existsSync(outDir)) {
    fail(`evidence directory already exists: ${outDir}`);
  }
  fs.mkdirSync(outDir, { recursive: true });
  const context = {
    generatedAt: new Date().toISOString(),
    outDir,
    quench,
    node,
    bun,
    rounds: options.rounds,
    host: hostIdentity(),
    git,
    versions: {
      node: commandOutput(node, ["--version"]),
      bun: commandOutput(bun, ["--version"]),
    },
    hashes: {
      quench: sha256(quench),
      node: sha256(node),
      bun: sha256(bun),
    },
    command: `node tools/npm-matrix.mjs --rounds ${options.rounds}` +
      ` --out ${path.relative(root, outDir)}`,
  };
  process.stderr.write(
    `npm-matrix: ${jobs.length} jobs on ${context.host.platform}/` +
    `${context.host.arch}, ${options.rounds} rounds -> ${outDir}\n`,
  );
  const rows = [];
  const failures = [];
  for (const [index, job] of jobs.entries()) {
    process.stderr.write(
      `[${index + 1}/${jobs.length}] ${job.workload.id} ${job.phase} ` +
      `(${job.rounds} ops, ${job.warmups} warmups)\n`,
    );
    const result = runJob(job, context);
    if (result.error) {
      failures.push({ job, error: result.error });
      process.stderr.write(`  failed: ${result.error.split("\n")[0]}\n`);
      continue;
    }
    const evaluated = evaluate(result);
    if (evaluated.error) {
      failures.push({ job, error: evaluated.error });
      process.stderr.write(`  failed: ${evaluated.error}\n`);
      continue;
    }
    rows.push(evaluated.row);
    const speed = formatRatio(evaluated.row.speedRatio);
    const rss = formatRatio(evaluated.row.rssRatio);
    const verdict = evaluated.row.pass
      ? "PASS"
      : `MISS ${formatNumber(evaluated.row.quench.operationsPerSecond)} ops/s, ` +
        `${formatMiB(evaluated.row.quench.maximumRssBytes)} MiB`;
    process.stderr.write(
      `  quench ${formatNumber(evaluated.row.quench.operationsPerSecond)} ops/s, ` +
      `${formatMiB(evaluated.row.quench.maximumRssBytes)} MiB; ${speed} speed, ` +
      `${rss} RSS: ${verdict}\n`,
    );
  }
  const summary = {
    schema: 1,
    generatedAt: context.generatedAt,
    qualificationReady: false,
    gate: GATE,
    host: context.host,
    git,
    toolchain: context.versions,
    binaries: { quench, node, bun },
    hashes: context.hashes,
    rounds: options.rounds,
    command: context.command,
    rows,
    failures,
    passed: rows.filter((row) => row.pass).length,
    total: rows.length,
  };
  fs.writeFileSync(
    path.join(outDir, "summary.json"),
    `${JSON.stringify(summary, null, 2)}\n`,
  );
  fs.writeFileSync(
    path.join(outDir, "README.md"),
    markdown(rows, failures, context),
  );
  process.stdout.write(
    `npm-matrix: ${summary.passed}/${summary.total} rows passed; ` +
    `evidence written to ${path.relative(root, outDir)}\n`,
  );
  if (options.gate && (failures.length > 0 || summary.passed !== summary.total)) {
    process.exit(1);
  }
}

main();

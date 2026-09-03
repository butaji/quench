#!/usr/bin/env node
"use strict";

// One control/diagnostic command: run an explicit artifact, retain raw samples,
// then derive its appendable EvidenceRecord. Never called by production code.
const cp = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const args = process.argv.slice(2);
const option = (name, fallback = null) => {
  const index = args.indexOf(name);
  return index >= 0 && index + 1 < args.length ? args[index + 1] : fallback;
};
const binaryValue = option("--binary", option("--quench"));
if (!binaryValue) throw new Error("missing --binary (an explicit optimized artifact is required)");

const binary = path.resolve(binaryValue);
const output = path.resolve(option("--out", path.join(root, "target", "v8-evidence.json")));
const raw = path.resolve(option("--raw-out", `${output}.runs.json`));
const lane = option("--lane", "control");
const runs = option("--runs", "5");
const timeout = option("--timeout-ms", "120000");
const only = option("--only");
const append = option("--append");
const identifier = option("--id");
const parent = option("--parent");
const artifactGitCommit = option("--artifact-git-commit");

fs.mkdirSync(path.dirname(output), { recursive: true });
const runnerArgs = [
  path.join(root, "quench-bench", "run-quench-runtime.mjs"),
  "--quench", binary, "--runs", runs, "--timeout-ms", timeout, "--out", raw,
];
if (only) runnerArgs.push("--only", only);
const run = cp.spawnSync(process.execPath, runnerArgs, { cwd: root, stdio: "inherit" });
if (run.status !== 0) process.exit(run.status ?? 1);

const recordArgs = [
  path.join(root, "tools", "evidence-record.cjs"),
  "--runs", raw, "--binary", binary, "--lane", lane, "--out", output,
];
if (!only) recordArgs.push("--full-v8");
if (append) recordArgs.push("--append", path.resolve(append));
if (identifier) recordArgs.push("--id", identifier);
if (parent) recordArgs.push("--parent", parent);
if (artifactGitCommit) recordArgs.push("--artifact-git-commit", artifactGitCommit);
const record = cp.spawnSync(process.execPath, recordArgs, { cwd: root, stdio: "inherit" });
process.exit(record.status ?? 1);

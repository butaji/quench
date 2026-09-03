#!/usr/bin/env node
"use strict";

const cp = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..");
const contractsPath = path.join(root, "quench-bench/profile-contracts.json");
const contracts = JSON.parse(fs.readFileSync(contractsPath, "utf8"));
const separator = process.argv.indexOf("--");
const requested = process.argv.slice(2, separator < 0 ? undefined : separator);
const forwarded = separator < 0 ? [] : process.argv.slice(separator + 1);
const benchmarks = requested.length ? requested : Object.keys(contracts.benchmarks);
const DEFAULT_TIMEOUT_MS = 120_000;
let failed = false;

for (const benchmark of benchmarks) {
  if (!contracts.benchmarks[benchmark]) {
    console.error(`unknown benchmark profile: ${benchmark}`);
    failed = true;
    continue;
  }
  const result = cp.spawnSync(process.execPath, [
    path.join(root, "tools/analyze-quench-bench.cjs"), benchmark,
    "--assert-profile", contractsPath, ...forwarded,
  ], {
    cwd: root,
    stdio: "inherit",
    timeout: DEFAULT_TIMEOUT_MS,
    killSignal: "SIGKILL",
  });
  failed ||= result.status !== 0;
}

process.exitCode = failed ? 1 : 0;

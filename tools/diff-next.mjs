#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { performance } from "node:perf_hooks";

const source = process.argv[2];
if (!source) {
  console.error("usage: diff-next.mjs SCRIPT");
  process.exit(2);
}

const absoluteSource = path.resolve(source);
if (!fs.existsSync(absoluteSource)) {
  console.error(`missing script: ${absoluteSource}`);
  process.exit(2);
}
const sourceSha256 = createHash("sha256").update(fs.readFileSync(absoluteSource)).digest("hex");

const entries = [
  ["legacy-quench", process.env.LEGACY_QUENCH_BIN ?? "target/debug/quench-node"],
  ["next-quench", process.env.NEXT_QUENCH_BIN ?? "target/debug/quench-next"],
  ["node-oracle", process.env.NODE_BIN ?? process.execPath],
];

function execute(label, command) {
  const started = performance.now();
  const result = spawnSync(command, [absoluteSource], {
    cwd: process.cwd(),
    encoding: "utf8",
    timeout: Number(process.env.DIFF_TIMEOUT_MS ?? 30_000),
    env: process.env,
  });
  const elapsed = performance.now() - started;
  const spawnError = result.error?.message ?? null;
  const timedOut = result.error?.code === "ETIMEDOUT";
  return {
    label,
    command,
    status: result.status,
    signal: result.signal,
    timed_out: timedOut,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    spawn_error: spawnError,
    duration_ms: Math.round(elapsed * 1000) / 1000,
  };
}

const results = entries.map(([label, command]) => execute(label, command));
const observable = ({ status, signal, timed_out, stdout, stderr, spawn_error }) =>
  JSON.stringify({ status, signal, timed_out, stdout, stderr, spawn_error });
const reference = observable(results[2]);

console.log(
  JSON.stringify(
    {
      schema: 1,
      source: absoluteSource,
      source_sha256: sourceSha256,
      timeout_ms: Number(process.env.DIFF_TIMEOUT_MS ?? 30_000),
      results,
      matches_node: results.slice(0, 2).map((result) => observable(result) === reference),
    },
    null,
    2,
  ),
);

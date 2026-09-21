#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { performance } from "node:perf_hooks";

function observable({ status, signal, timed_out, stdout, stderr, spawn_error }) {
  return JSON.stringify({ status, signal, timed_out, stdout, stderr, spawn_error });
}

function semanticStderr(stderr) {
  return stderr
    .split(/\r?\n/)
    .filter((line) => {
      if (!line) return false;
      try {
        const record = JSON.parse(line);
        return !(typeof record.kind === "string" && record.kind.startsWith("rqj-"));
      } catch {
        return true;
      }
    })
    .join("\n");
}

function semanticObservable(result) {
  return observable({ ...result, stderr: semanticStderr(result.stderr) });
}

const source = process.argv[2];
const selfTest = source === "--self-test";
if (selfTest) {
  const assert = (condition, message) => {
    if (!condition) throw new Error(`self-test failed: ${message}`);
  };
  const executeSelfTest = (label, args, timeout) => execute(label, process.execPath, args, timeout);
  const timeout = executeSelfTest("timeout", ["-e", "setTimeout(() => {}, 1000)"], 10);
  assert(timeout.timed_out, "timeout is classified");
  const crash = executeSelfTest("crash", ["-e", "process.exit(7)"], 1000);
  assert(crash.status === 7 && !crash.timed_out, "nonzero exit is preserved");
  const measured = { status: 0, signal: null, timed_out: false, stdout: "42\n", stderr: '{"kind":"rqj-profile"}\n', spawn_error: null };
  const clean = { ...measured, stderr: "" };
  assert(semanticObservable(measured) === semanticObservable(clean), "measurement stderr is non-semantic");
  const values = [timeout, crash].map(observable);
  assert(values[0] !== values[1], "observable mismatches remain distinguishable");
  console.log("diff-next self-test: ok");
  process.exit(0);
}
if (!source) {
  console.error("usage: diff-next.mjs SCRIPT | --self-test");
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

function execute(label, command, args = [absoluteSource], timeout = Number(process.env.DIFF_TIMEOUT_MS ?? 30_000)) {
  const started = performance.now();
  const result = spawnSync(command, args, {
    cwd: process.cwd(),
    encoding: "utf8",
    timeout,
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
const reference = semanticObservable(results[2]);

console.log(
  JSON.stringify(
    {
      schema: 1,
      source: absoluteSource,
      source_sha256: sourceSha256,
      timeout_ms: Number(process.env.DIFF_TIMEOUT_MS ?? 30_000),
      results,
      matches_node: results.slice(0, 2).map((result) => semanticObservable(result) === reference),
    },
    null,
    2,
  ),
);

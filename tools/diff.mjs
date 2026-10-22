#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { performance } from "node:perf_hooks";

const engineRoles = {
  oracle: "node-oracle",
  specialized: "quench-specialized",
  generic: "quench-generic",
};

function completed({ status, signal, timed_out, spawn_error }) {
  return Number.isInteger(status) && signal === null && timed_out === false &&
    spawn_error === null;
}

function matches(left, right) {
  return completed(left) && completed(right) &&
    semanticObservable(left) === semanticObservable(right);
}

function observable(
  { status, signal, timed_out, stdout, stderr, spawn_error },
) {
  return JSON.stringify({
    status,
    signal,
    timed_out,
    stdout,
    stderr,
    spawn_error,
  });
}

function semanticStderr(stderr) {
  return stderr
    .split(/\r?\n/)
    .filter((line) => {
      if (!line) return false;
      try {
        const record = JSON.parse(line);
        return !(typeof record.kind === "string" &&
          record.kind.startsWith("quench-"));
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
const timeoutMs = Number(process.env.DIFF_TIMEOUT_MS);
if (selfTest) {
  const assert = (condition, message) => {
    if (!condition) throw new Error(`self-test failed: ${message}`);
  };
  const executeSelfTest = (label, args, timeout) =>
    execute(label, process.execPath, args, timeout);
  const timeout = executeSelfTest("timeout", [
    "-e",
    "setTimeout(() => {}, 1000)",
  ], 10);
  assert(timeout.timed_out, "timeout is classified");
  const crash = executeSelfTest("crash", ["-e", "process.exit(7)"], 1000);
  assert(crash.status === 7 && !crash.timed_out, "nonzero exit is preserved");
  const measured = {
    status: 0,
    signal: null,
    timed_out: false,
    stdout: "42\n",
    stderr: '{"kind":"quench-profile"}\n',
    spawn_error: null,
  };
  const clean = { ...measured, stderr: "" };
  assert(
    matches(measured, clean),
    "measurement stderr is non-semantic",
  );
  assert(!matches(timeout, timeout), "matching timeouts are not verification");
  for (
    const invalid of [
      { ...clean, status: null, signal: "SIGABRT" },
      { ...clean, status: null, spawn_error: "missing executable" },
      { ...clean, status: null },
      { ...clean, timed_out: undefined },
    ]
  ) {
    assert(!matches(invalid, invalid), "incomplete observations never match");
  }
  const expectedExit = { ...clean, status: 7 };
  assert(
    matches(expectedExit, expectedExit),
    "observed nonzero exits still compare",
  );
  assert(!matches(expectedExit, clean), "different exit statuses do not match");
  const values = [timeout, crash].map(observable);
  assert(
    values[0] !== values[1],
    "observable mismatches remain distinguishable",
  );
  console.log("diff self-test: ok");
  process.exit(0);
}
if (!source) {
  console.error("usage: diff.mjs SCRIPT | --self-test");
  process.exit(2);
}
if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
  console.error(
    "DIFF_TIMEOUT_MS must be set to a positive timeout in milliseconds",
  );
  process.exit(2);
}

const absoluteSource = path.resolve(source);
if (!fs.existsSync(absoluteSource)) {
  console.error(`missing script: ${absoluteSource}`);
  process.exit(2);
}
const sourceSha256 = createHash("sha256").update(
  fs.readFileSync(absoluteSource),
).digest("hex");

const entries = [
  {
    label: "quench-node",
    command: process.env.QUENCH_NODE_BIN ?? "target/debug/quench-node",
    args: () => [absoluteSource],
  },
  {
    label: engineRoles.specialized,
    command: process.env.QUENCH_BIN ?? "target/debug/quench",
    args: () => [absoluteSource],
  },
  {
    label: engineRoles.generic,
    command: process.env.QUENCH_BIN ?? "target/debug/quench",
    args: () => ["--generic", absoluteSource],
  },
  {
    label: engineRoles.oracle,
    command: process.env.NODE_BIN ?? process.execPath,
    args: () => [absoluteSource],
  },
];

function execute(
  label,
  command,
  args,
  timeout = timeoutMs,
) {
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

const results = entries.map((entry) =>
  execute(entry.label, entry.command, entry.args())
);
const reference = results.find((result) => result.label === engineRoles.oracle);
const optimized = results.find((result) =>
  result.label === engineRoles.specialized
);
const generic = results.find((result) => result.label === engineRoles.generic);

console.log(
  JSON.stringify(
    {
      schema: 2,
      source: absoluteSource,
      source_sha256: sourceSha256,
      timeout_ms: timeoutMs,
      results,
      observation_complete: results.every(completed),
      matches_node: results.filter((result) =>
        result.label !== engineRoles.oracle
      )
        .map((result) => matches(result, reference)),
      matches_quench_modes: matches(generic, optimized),
    },
    null,
    2,
  ),
);

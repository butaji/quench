#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

const root = process.argv[2];
if (!root || root === "--help" || root === "-h") {
  console.error("usage: run-all-next.mjs DIRECTORY");
  process.exit(root ? 0 : 2);
}

const directory = path.resolve(root);
if (!fs.existsSync(directory) || !fs.statSync(directory).isDirectory()) {
  console.error(`missing fixture directory: ${directory}`);
  process.exit(2);
}

function discover(current, output = []) {
  for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
    const full = path.join(current, entry.name);
    if (entry.isDirectory()) discover(full, output);
    else if (entry.isFile() && /\.(?:js|mjs)$/.test(entry.name)) output.push(full);
  }
  return output;
}

const sources = discover(directory).sort((left, right) => left.localeCompare(right));
if (!sources.length) {
  console.error(`no JavaScript fixtures found under: ${directory}`);
  process.exit(2);
}

const runner = path.resolve("tools/diff-next.mjs");
const timeout = Number(process.env.DIFF_TIMEOUT_MS ?? 30_000);
const records = [];
for (const source of sources) {
  const result = spawnSync(process.execPath, [runner, source], {
    cwd: process.cwd(),
    encoding: "utf8",
    timeout: timeout + 5_000,
    env: process.env,
  });
  let record;
  try {
    record = JSON.parse(result.stdout);
  } catch {
    record = {
      schema: 1,
      source,
      runner_status: result.status,
      runner_signal: result.signal,
      runner_error: result.error?.message ?? null,
      stdout: result.stdout ?? "",
      stderr: result.stderr ?? "",
    };
  }
  records.push(record);
}

const mismatches = records.filter((record) =>
  Array.isArray(record.matches_node) && record.matches_node.some((match) => !match),
).length;
console.log(
  JSON.stringify(
    {
      schema: 1,
      root: directory,
      fixture_count: sources.length,
      deterministic_order: sources.map((source) => path.relative(directory, source)),
      mismatch_count: mismatches,
      records,
    },
    null,
    2,
  ),
);
process.exit(mismatches === 0 ? 0 : 1);

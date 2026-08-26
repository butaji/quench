"use strict";

const cp = require("node:child_process");
const path = require("node:path");

const runner = path.join(__dirname, "..", "run-with-timeout.cjs");
const DEFAULT_TIMEOUT_MS = 180_000;

function deadline(value = process.env.QUENCH_RUN_TIMEOUT_MS) {
  const milliseconds = Number(value || DEFAULT_TIMEOUT_MS);
  if (!Number.isFinite(milliseconds) || milliseconds <= 0)
    throw new Error(`invalid run timeout: ${value}`);
  return milliseconds;
}

function spawnSync(command, args = [], options = {}) {
  const milliseconds = deadline(options.deadlineMs);
  const spawnOptions = { ...options };
  delete spawnOptions.deadlineMs;
  return cp.spawnSync(
    process.execPath,
    [runner, String(milliseconds), command, ...args],
    { timeout: milliseconds + 10_000, ...spawnOptions }
  );
}

module.exports = { DEFAULT_TIMEOUT_MS, deadline, spawnSync };

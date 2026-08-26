#!/usr/bin/env node
"use strict";

const { spawn } = require("node:child_process");

const [, , timeoutText, command, ...args] = process.argv;
const timeoutMs = Number(timeoutText);
if (!command || !Number.isFinite(timeoutMs) || timeoutMs <= 0) {
  console.error(
    "usage: node tools/run-with-timeout.cjs TIMEOUT_MS COMMAND [ARG ...]"
  );
  process.exit(2);
}

const grouped = process.platform !== "win32";
const child = spawn(command, args, {
  detached: grouped,
  env: process.env,
  stdio: ["ignore", "pipe", "pipe"]
});
child.stdout.pipe(process.stdout);
child.stderr.pipe(process.stderr);

let timedOut = false;
function terminate(signal = "SIGKILL") {
  if (!child.pid) return;
  try {
    process.kill(grouped ? -child.pid : child.pid, signal);
  } catch (error) {
    if (error.code !== "ESRCH") throw error;
  }
}

const timer = setTimeout(() => {
  timedOut = true;
  terminate();
}, timeoutMs);

for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.once(signal, () => {
    terminate(signal);
    process.exit(
      128 + (signal === "SIGINT" ? 2 : signal === "SIGTERM" ? 15 : 1)
    );
  });
}

child.once("error", (error) => {
  clearTimeout(timer);
  console.error(error.message);
  process.exit(127);
});
child.once("close", (code, signal) => {
  clearTimeout(timer);
  if (timedOut) {
    console.error(`command timed out after ${timeoutMs}ms: ${command}`);
    process.exit(124);
  }
  if (signal) {
    console.error(`command terminated by ${signal}: ${command}`);
    process.exit(128);
  }
  process.exit(code ?? 1);
});

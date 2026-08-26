#!/usr/bin/env node
"use strict";

const cp = require("node:child_process");
const path = require("node:path");

const runner = path.join(__dirname, "run-with-timeout.cjs");
const started = Date.now();
const result = cp.spawnSync(
  process.execPath,
  [runner, "100", process.execPath, "-e", "setInterval(() => {}, 1000)"],
  {
    encoding: "utf8",
    timeout: 2000
  }
);
if (result.status !== 124)
  throw new Error(
    `expected timeout status 124, got ${result.status}\n${result.stderr}`
  );
if (Date.now() - started > 1500)
  throw new Error("timeout runner exceeded its deadline budget");
if (!result.stderr.includes("command timed out after 100ms"))
  throw new Error("timeout diagnostic missing");
console.log("run-with-timeout: ok");

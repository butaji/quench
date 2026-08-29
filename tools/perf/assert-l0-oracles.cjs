#!/usr/bin/env node
"use strict";

const cp = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const root = path.resolve(__dirname, "..", "..");
const lanes = path.join(root, "tests/lanes");
const quench = path.resolve(root, process.env.QUENCH_SCORE_BINARY ||
  "target/bench-throughput/quench-node");
const oracleSource = path.join(root, "tools/perf/l0-oracles.rs");
const oracle = path.join(root, "target/l0-oracles");

function buildOracle() {
  const result = cp.spawnSync("rustc", [
    oracleSource, "--edition=2021", "-C", "opt-level=3", "-C", "lto=fat",
    "-C", "codegen-units=1", "-o", oracle,
  ], { encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr || "rustc failed");
}

function elapsed(command, args) {
  const start = process.hrtime.bigint();
  const result = cp.spawnSync(command, args, {
    encoding: "utf8",
    timeout: 120_000,
  });
  const ns = Number(process.hrtime.bigint() - start);
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed (${result.status})\n${result.stderr || ""}`);
  }
  return ns;
}

function median(values) {
  const ordered = values.slice().sort((left, right) => left - right);
  return ordered[Math.floor(ordered.length / 2)];
}

function measure(id) {
  const source = path.join(lanes, `${id}.js`);
  const want = JSON.parse(fs.readFileSync(path.join(lanes, `${id}.want.json`), "utf8"));
  if (!want.oracle) throw new Error(`${id}: missing oracle contract`);
  elapsed(oracle, [id]);
  elapsed(quench, [source]);
  const oracleNs = median(Array.from({ length: 5 }, () => elapsed(oracle, [id])));
  const jsNs = median(Array.from({ length: 5 }, () => elapsed(quench, [source])));
  const ratio = jsNs / oracleNs;
  const passed = ratio <= want.oracle.max_ratio;
  process.stdout.write(`${JSON.stringify({ id, passed, ratio, js_ns: jsNs, oracle_ns: oracleNs })}\n`);
  if (!passed) console.error(`${id}.oracle_ratio: ${ratio}; above ${want.oracle.max_ratio}`);
  return passed;
}

function main() {
  buildOracle();
  const requested = process.argv.slice(2);
  const ids = requested.length ? requested : fs.readdirSync(lanes)
    .filter((name) => name.endsWith(".want.json"))
    .map((name) => name.slice(0, -".want.json".length))
    .filter((id) => JSON.parse(fs.readFileSync(path.join(lanes, `${id}.want.json`), "utf8")).oracle);
  let passed = true;
  for (const id of ids) passed = measure(id) && passed;
  process.exitCode = passed ? 0 : 1;
}

if (require.main === module) main();

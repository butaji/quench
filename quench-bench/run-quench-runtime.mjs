#!/usr/bin/env node
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";

const root = resolve(import.meta.dirname, "..");
const suite = resolve(import.meta.dirname, "js-engine-benchmark", "v8-v7");
const runtime = resolve(root, "target", "release", "quench-node");
const fixtures = ["richards.js", "deltablue.js", "crypto.js", "raytrace.js", "earley-boyer.js", "regexp.js", "splay.js", "navier-stokes.js"];

function usage() {
  console.log("usage: node quench-bench/run-quench-runtime.mjs [--node PATH] [--quench PATH] [--runs N]");
}
function arg(name, fallback) {
  const i = process.argv.indexOf(name);
  return i >= 0 && process.argv[i + 1] ? process.argv[i + 1] : fallback;
}
if (process.argv.includes("--help")) { usage(); process.exit(0); }
const node = arg("--node", process.execPath);
const quench = arg("--quench", runtime);
const runs = Number(arg("--runs", "3"));

function measure(command, script) {
  const start = process.hrtime.bigint();
  const result = spawnSync(command, [script], { encoding: "utf8" });
  const wallNs = Number(process.hrtime.bigint() - start);
  return { command, script, status: result.status, wallNs, stdout: result.stdout, stderr: result.stderr };
}

if (!existsSync(suite)) throw new Error(`benchmark suite not found: ${suite}`);
for (const fixture of fixtures) {
  const script = join(suite, fixture);
  const base = readFileSync(join(suite, "base.js"), "utf8");
  const source = base + "\n" + readFileSync(script, "utf8");
  const temp = join("/tmp", `quench-bench-${fixture}`);
  writeFileSync(temp, source);
  const nodeRuns = Array.from({ length: runs }, () => measure(node, temp));
  const quenchRuns = Array.from({ length: runs }, () => measure(quench, temp));
  console.log(JSON.stringify({ fixture, node: nodeRuns.map(({ status, wallNs }) => ({ status, wallNs })), quench: quenchRuns.map(({ status, wallNs }) => ({ status, wallNs })), outputEqual: nodeRuns.at(-1).stdout === quenchRuns.at(-1).stdout }));
}

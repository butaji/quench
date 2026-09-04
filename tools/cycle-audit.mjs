#!/usr/bin/env node

// Task 056 measurement-only probe.  It deliberately runs the VM in a fresh
// process for each size and reads peak RSS from macOS /usr/bin/time; no
// benchmark fixture or source identity is inspected by production code.
import { spawnSync } from "node:child_process";

const engineIndex = process.argv.indexOf("--engine");
const engine = engineIndex >= 0 ? process.argv[engineIndex + 1] : "target/debug/quench-node";
if (!engine) throw new Error("--engine requires a path");

const probes = [
  ["plain_object", n => `for (var i=0;i<${n};i++){var a={};var b={};a.peer=b;b.peer=a;}`],
  ["closure_capture", n => `function make(){var a={};function f(){return a;}a.f=f;}for(var i=0;i<${n};i++)make();`],
  ["constraint_variable", n => `function Constraint(){this.v=null;this.next=null;}for(var i=0;i<${n};i++){var c=new Constraint();var v={constraint:c};c.v=v;c.next=c;}`],
];

for (const [name, makeSource] of probes) {
  for (const n of [1_000, 10_000, 50_000]) {
    const result = spawnSync("/usr/bin/time", ["-l", engine, "-e", makeSource(n)], {
      encoding: "utf8",
      maxBuffer: 4 * 1024 * 1024,
    });
    const report = `${result.stdout ?? ""}\n${result.stderr ?? ""}`;
    const rss = report.match(/(\d+)\s+maximum resident set size/)?.[1]
      ?? report.match(/(\d+)\s+peak memory footprint/)?.[1]
      ?? "unknown";
    console.log(`${name} N=${n} peak_rss_bytes=${rss} status=${result.status ?? "signal"}`);
  }
}

#!/usr/bin/env node
// Per-operation cost probes for the ten-package npm workloads.
//
// The package matrix says whether a row wins; it does not say what one
// operation of a package's hot path costs. These probes isolate a single
// operation (`new Response(buffer)`, `new Headers()`, a route match, a call)
// and report retired instructions and cycles per operation for Quench and
// each peer, so a change to the runtime is attributed to the operation that
// carries it.
//
// `npm-matrix.mjs` remains the row gate; this tool never claims a package win.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// Probes run from the workload directory so the pinned package graph resolves.
const probeDir = path.join(root, "tests/npm-perf");

// Each probe is a loop over `n` operations of one part; `sink` keeps the
// operation live so no engine can elide it.
const PROBES = [
  {
    id: "call-2-args",
    package: "engine",
    source: `
function f(a, b) { return (a + b) | 0; }
let s = 0;
for (let i = 0; i < n; i++) s = (s + f(s, i)) | 0;
console.log(s);`,
  },
  {
    id: "headers-new",
    package: "Headers",
    source: `
let s = 0;
for (let i = 0; i < n; i++) { const h = new Headers(); h.set("content-type", "application/json"); s += h.get("content-type").length; }
console.log(s);`,
  },
  {
    id: "readable-stream-new",
    package: "ReadableStream",
    source: `
let s = 0;
for (let i = 0; i < n; i++) { const r = new ReadableStream({ start(c) { c.enqueue(new Uint8Array(8)); c.close(); } }); s += r.locked ? 0 : 1; }
console.log(s);`,
  },
  {
    id: "response-null",
    package: "Response",
    source: `
let s = 0;
for (let i = 0; i < n; i++) { const r = new Response(null); s += r.status; }
console.log(s);`,
  },
  {
    id: "response-buffer",
    package: "Response",
    source: `
const buf = Buffer.from("hello world payload", "utf8");
let s = 0;
for (let i = 0; i < n; i++) { const r = new Response(buf); s += r.status; }
console.log(s);`,
  },
  {
    id: "nextresponse-json",
    package: "Next",
    source: `
const { NextResponse } = require("next/server");
let s = 0;
for (let i = 0; i < n; i++) { const r = NextResponse.json({ route: 1, params: { slug: ["a", "b"] } }, { status: 200 }); s += r.status; }
console.log(s);`,
  },
  {
    id: "next-route-match",
    package: "Next",
    source: `
const { getRouteMatcher } = require("next/dist/shared/lib/router/utils/route-matcher");
const { getRouteRegex } = require("next/dist/shared/lib/router/utils/route-regex");
const routes = [
  ["/products/[category]/[...slug]", "/products/books/runtime/performance"],
  ["/docs/[...slug]", "/docs/api/server/route-handlers"],
  ["/users/[id]", "/users/8421"],
].map(([route, pathname]) => ({ match: getRouteMatcher(getRouteRegex(route)), pathname }));
let s = 0;
for (let i = 0; i < n; i++) { const r = routes[i % routes.length]; s += r.match(r.pathname) ? 1 : 0; }
console.log(s);`,
  },
  {
    id: "json-stringify",
    package: "JSON",
    source: `
let s = 0;
for (let i = 0; i < n; i++) s += JSON.stringify({ route: i % 4, params: { slug: ["a", "b"] } }).length;
console.log(s);`,
  },
];

const ENGINES = [
  { id: "quench", binary: "quench", args: [] },
  { id: "quench-jit", binary: "quench", args: ["--jit"] },
  { id: "node-jitless", binary: "node", args: ["--jitless"] },
  { id: "node", binary: "node", args: [] },
  { id: "bun-no-jit", binary: "bun", args: [], env: { BUN_JSC_useJIT: "0" } },
  { id: "bun", binary: "bun", args: [] },
];

function usage() {
  process.stdout.write(`usage: node tools/npm-cost-probes.mjs [options]

Options:
  --ops N             operations per probe (default 20000)
  --repeat N          runs per probe and engine, median reported (default 3)
  --probe ID          repeatable probe id (default: all)
  --quench PATH       quench-node binary (default target/production/quench-node)
  --node PATH         Node binary (default the running node)
  --bun PATH          Bun binary (default $BUN_BIN, then bun on PATH)
  --json PATH         write the measurements as JSON
  --help              this text

Probes: ${PROBES.map((probe) => probe.id).join(", ")}

Retired instructions and cycles come from /usr/bin/time -l (Darwin). The
counters cover the whole process, startup included, so read the per-operation
column as cost with a small constant; compare engines within one run only.
`);
}

function parseArgs(argv) {
  const options = { ops: 20000, repeat: 3, probes: new Set(), json: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const value = () => argv[++index];
    switch (argv[index]) {
      case "--ops": options.ops = Number(value()); break;
      case "--repeat": options.repeat = Number(value()); break;
      case "--probe": options.probes.add(value()); break;
      case "--quench": options.quench = value(); break;
      case "--node": options.node = value(); break;
      case "--bun": options.bun = value(); break;
      case "--json": options.json = value(); break;
      case "--help": case "-h": usage(); process.exit(0); break;
      default: process.stderr.write(`npm-cost-probes: unknown option ${argv[index]}\n`); process.exit(2);
    }
  }
  if (!Number.isSafeInteger(options.repeat) || options.repeat < 1 || options.repeat > 15) {
    process.stderr.write("npm-cost-probes: --repeat must be between 1 and 15\n");
    process.exit(2);
  }
  if (!Number.isSafeInteger(options.ops) || options.ops < 1000) {
    process.stderr.write("npm-cost-probes: --ops must be an integer of at least 1000\n");
    process.exit(2);
  }
  return options;
}

function resolveBinary(explicit, candidates) {
  if (explicit) return explicit.includes("/") ? path.resolve(explicit) : explicit;
  for (const candidate of candidates) {
    if (!candidate) continue;
    if (candidate.includes("/")) {
      if (fs.existsSync(candidate)) return candidate;
      continue;
    }
    const probe = spawnSync("which", [candidate], { encoding: "utf8" });
    if (probe.status === 0) return probe.stdout.trim();
  }
  return candidates.filter(Boolean).at(-1);
}

function measure(command, args, env) {
  const timed = process.platform === "darwin"
    ? { command: "/usr/bin/time", args: ["-l", command, ...args] }
    : { command, args };
  const result = spawnSync(timed.command, timed.args, {
    cwd: probeDir,
    encoding: "utf8",
    env: { ...process.env, ...env },
    timeout: 600_000,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} exited ${result.status ?? result.signal}:\n${result.stderr}`);
  }
  if (process.platform !== "darwin") {
    return { instructions: null, cycles: null, elapsedMs: result.elapsed ?? null, stderr: result.stderr };
  }
  const count = (label) => {
    const match = result.stderr.match(new RegExp(`([\\d,]+)\\s+${label}`));
    return match ? Number(match[1].replaceAll(",", "")) : null;
  };
  return {
    instructions: count("instructions retired"),
    cycles: count("cycles elapsed"),
    rssBytes: count("maximum resident set size"),
    stderr: result.stderr,
  };
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (process.platform !== "darwin") {
    process.stderr.write("npm-cost-probes: retired instructions need /usr/bin/time -l (Darwin); " +
      "measuring wall time only, which is not comparable across engines\n");
  }
  const binaries = {
    quench: resolveBinary(options.quench, [path.join(root, "target/production/quench-node")]),
    node: resolveBinary(options.node, [process.execPath]),
    bun: resolveBinary(options.bun, [process.env.BUN_BIN, "bun", path.join(os.homedir(), ".bun/bin/bun")]),
  };
  if (!fs.existsSync(binaries.quench)) {
    process.stderr.write(`npm-cost-probes: quench binary not found at ${binaries.quench}\n`);
    process.exit(2);
  }
  const probes = options.probes.size === 0
    ? PROBES
    : PROBES.filter((probe) => options.probes.has(probe.id));
  if (probes.length === 0) {
    process.stderr.write("npm-cost-probes: no probes selected\n");
    process.exit(2);
  }
  const scratch = fs.mkdtempSync(path.join(probeDir, ".cost-probes-"));
  const measurements = [];
  try {
    for (const probe of probes) {
      const file = path.join(scratch, `${probe.id}.cjs`);
      fs.writeFileSync(file, `const n = ${options.ops};\n${probe.source}\n`);
      for (const engine of ENGINES) {
        const runs = [];
        for (let repeat = 0; repeat < options.repeat; repeat += 1) {
          runs.push(measure(binaries[engine.binary], [...engine.args, file], engine.env));
        }
        // Retired instructions vary by a tenth between runs on a shared host, so a
        // single run cannot separate a change from noise; report the median.
        const measured = {
          instructions: median(runs.map((run) => run.instructions)),
          cycles: median(runs.map((run) => run.cycles)),
          rssBytes: median(runs.map((run) => run.rssBytes)),
        };
        const perOperation = (value) => (value === null ? null : Number((value / options.ops).toFixed(1)));
        measurements.push({
          probe: probe.id,
          package: probe.package,
          engine: engine.id,
          ops: options.ops,
          repeats: options.repeat,
          instructions: measured.instructions,
          cycles: measured.cycles,
          instructionsPerOperation: perOperation(measured.instructions),
          cyclesPerOperation: perOperation(measured.cycles),
          rssBytes: measured.rssBytes ?? null,
        });
        const row = measurements.at(-1);
        process.stdout.write(
          `${probe.id.padEnd(22)} ${engine.id.padEnd(12)} ` +
          `ins/op=${format(row.instructionsPerOperation)} cyc/op=${format(row.cyclesPerOperation)} ` +
          `rss=${formatMiB(row.rssBytes)}\n`,
        );
      }
      process.stdout.write("\n");
    }
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
  if (options.json) {
    fs.writeFileSync(options.json, `${JSON.stringify({
      schema: 1,
      generatedAt: new Date().toISOString(),
      ops: options.ops,
      repeats: options.repeat,
      host: { platform: process.platform, arch: process.arch },
      binaries,
      measurements,
    }, null, 1)}\n`);
    process.stdout.write(`wrote ${options.json}\n`);
  }
}

function median(values) {
  const numbers = values.filter((value) => value !== null && value !== undefined).sort((left, right) => left - right);
  if (numbers.length === 0) return null;
  const middle = Math.floor(numbers.length / 2);
  return numbers.length % 2 ? numbers[middle] : Math.round((numbers[middle - 1] + numbers[middle]) / 2);
}

function format(value) {
  return value === null || value === undefined ? "n/a" : String(value);
}

function formatMiB(bytes) {
  return bytes === null || bytes === undefined ? "n/a" : `${(bytes / 2 ** 20).toFixed(1)}MiB`;
}

main();

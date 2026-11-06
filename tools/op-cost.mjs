#!/usr/bin/env node
// Operation cost matrix: steady-state instructions per JavaScript operation, per tier.
//
// Each operation is one loop body run inside a function. A run warms the function, then
// executes N iterations; the cost of one operation is the callgrind instruction difference
// between a 2N and an N run, divided by N, so start-up, compilation and warm-up cancel.
// Every fixture prints a checksum, which must match Node's in every tier: one table checks
// semantics and cost together. The C column is the work a C programmer's loop would do for
// the same body (loop control included), the target the native tiers are judged against.
//
// Usage: tools/op-cost.mjs <quench-node> [--tiers interp,jit,lane] [--baseline file.json]
//        [--save file.json] [ops...]

import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// name, setup (top-level code), the names `run` takes from it, body (per iteration, `i` the
// index, `acc` the accumulator), C instructions per iteration. Setup runs outside the measured
// function, so the function is the loop and its body alone.
const OPERATIONS = [
  ["int-add", "", [], "acc = (acc + i) | 0;", 3],
  ["float-add", "var x = 0.5;", ["x"], "acc = acc + x;", 3],
  ["float-mul-add", "", [], "acc = acc * 0.999 + i;", 5],
  ["int-compare", "var k = 1000;", ["k"], "if ((i & 2047) < k) acc = (acc + 1) | 0;", 5],
  ["bitwise", "", [], "acc = (acc ^ (i << 3)) >>> 1;", 5],
  ["field-load", "var o = { v: 3, w: 4 };", ["o"], "acc = (acc + o.v) | 0;", 4],
  ["field-store", "var o = { v: 3, w: 4 };", ["o"], "o.v = i; acc = (acc + o.w) | 0;", 5],
  ["proto-load", "function P() {} P.prototype.k = 5; var o = new P();", ["o"], "acc = (acc + o.k) | 0;", 4],
  ["poly-load", "var os = [{ v: 1 }, { u: 0, v: 2 }];", ["os"], "acc = (acc + os[i & 1].v) | 0;", 8],
  ["element-read", "var a = []; for (var j = 0; j < 1024; j++) a.push(j);", ["a"], "acc = (acc + a[i & 1023]) | 0;", 5],
  ["element-write", "var a = []; for (var j = 0; j < 1024; j++) a.push(j);", ["a"], "a[i & 1023] = i; acc = (acc + 1) | 0;", 5],
  ["call", "function f(x) { return (x + 1) | 0; }", ["f"], "acc = f(acc);", 3],
  [
    "method-call",
    "function C() { this.n = 1; } C.prototype.f = function (x) { return (x + this.n) | 0; }; var o = new C();",
    ["o"],
    "acc = o.f(acc);",
    5,
  ],
  [
    "closure",
    "var bump = (function () { var c = 0; return function () { c = (c + 1) | 0; return c; }; })();",
    ["bump"],
    "acc = (acc + bump()) | 0;",
    5,
  ],
  ["allocate", "", [], "var p = { x: i, y: acc }; acc = (p.x + p.y) | 0;", 4],
];

const WARM = 3;
const N = 20_000;

function fixture(setup, names, body, n) {
  // `acc` is printed as a checksum; doubles keep a fixed rendering through `toFixed`.
  const params = ["n", ...names].join(", ");
  const args = names.map((name) => `, ${name}`).join("");
  return `${setup}
function run(${params}) { var acc = 0; for (var i = 0; i < n; i++) { ${body} } return acc; }
for (var w = 0; w < ${WARM}; w++) run(${N}${args});
var r = run(${n}${args});
console.log(typeof r === "number" && !Number.isInteger(r) ? r.toFixed(6) : String(r));
`;
}

const ALL_TIERS = [
  ["interp", [], {}],
  ["jit", ["--jit"], {}],
  ["lane", ["--jit"], { QUENCH_JS_JIT_LANE_METHODS: "1" }],
  // The lane-body compiler alone: every loop in its function's body, no loop region.
  ["body", ["--jit"], { QUENCH_JS_JIT_LANE_METHODS: "1", QUENCH_JS_JIT_LOOP_REGIONS: "0" }],
];

function run(command, args, env) {
  return new Promise((resolve) => {
    const child = spawn(command, args, {
      env: { ...process.env, QUENCH_COMPILE_CACHE: "", ...env },
    });
    let out = "";
    let err = "";
    child.stdout.on("data", (d) => (out += d));
    child.stderr.on("data", (d) => (err += d));
    child.on("close", (code) => resolve({ code, out: out.trim(), err }));
  });
}

async function instructions(quench, args, env, file) {
  const { err } = await run(
    "valgrind",
    ["--tool=callgrind", "--callgrind-out-file=/dev/null", quench, ...args, file],
    env,
  );
  const match = err.match(/refs:\s*([\d,]+)/);
  return match ? Number(match[1].replaceAll(",", "")) : NaN;
}

// A bounded pool: the host's cores run the valgrind jobs.
async function pool(jobs, width) {
  const results = new Array(jobs.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: width }, async () => {
      while (next < jobs.length) {
        const index = next++;
        results[index] = await jobs[index]();
      }
    }),
  );
  return results;
}

const argv = process.argv.slice(2);
const quench = argv.shift();
const option = (name) => {
  const at = argv.indexOf(name);
  return at < 0 ? undefined : argv.splice(at, 2)[1];
};
const tierNames = option("--tiers");
const baselineFile = option("--baseline");
const saveFile = option("--save");
const TIERS = tierNames ? ALL_TIERS.filter(([tier]) => tierNames.split(",").includes(tier)) : ALL_TIERS;
const chosen = argv.length ? OPERATIONS.filter(([name]) => argv.includes(name)) : OPERATIONS;
const directory = mkdtempSync(join(tmpdir(), "op-cost-"));
const width = Number(execFileSync("nproc").toString().trim());

const jobs = [];
const cells = {};
for (const [name, setup, names, body] of chosen) {
  const single = join(directory, `${name}-1.js`);
  const double = join(directory, `${name}-2.js`);
  writeFileSync(single, fixture(setup, names, body, N));
  writeFileSync(double, fixture(setup, names, body, 2 * N));
  const expected = execFileSync("node", [single]).toString().trim();
  cells[name] = { expected };
  for (const [tier, args, env] of TIERS) {
    jobs.push(async () => {
      const result = await run(quench, [...args, single], env);
      const ok = result.code === 0 && result.out === expected;
      const [a, b] = await Promise.all([
        instructions(quench, args, env, single),
        instructions(quench, args, env, double),
      ]);
      cells[name][tier] = { ok, cost: (b - a) / N, got: result.out };
    });
  }
}
await pool(jobs, width);

const baseline = baselineFile ? JSON.parse(readFileSync(baselineFile, "utf8")) : {};
const column = (name, tier) => {
  const cell = cells[name][tier];
  const base = baseline[name]?.[tier]?.cost;
  const value = cell.cost.toFixed(1);
  const delta = base === undefined ? "" : ` (${cell.cost - base >= 0 ? "+" : ""}${(cell.cost - base).toFixed(1)})`;
  return `${cell.ok ? "" : "WRONG "}${value}${delta}`;
};
console.log(`| operation | C | ${TIERS.map(([tier]) => tier).join(" | ")} |`);
console.log(`|---|---|${TIERS.map(() => "---").join("|")}|`);
for (const [name, , , , c] of chosen) {
  console.log(`| ${name} | ${c} | ${TIERS.map(([tier]) => column(name, tier)).join(" | ")} |`);
}
const wrong = chosen.flatMap(([name]) =>
  TIERS.filter(([tier]) => !cells[name][tier].ok).map(([tier]) =>
    `${name}/${tier}: expected ${cells[name].expected}, got ${cells[name][tier].got}`
  )
);
for (const line of wrong) console.log(`WRONG ${line}`);
if (saveFile) writeFileSync(saveFile, JSON.stringify(cells, null, 1));
process.exitCode = wrong.length ? 1 : 0;

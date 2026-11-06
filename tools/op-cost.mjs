#!/usr/bin/env node
// Operation cost matrix: steady-state instructions per JavaScript operation, per engine mode.
//
// Each operation is one loop body run inside a function. A run warms the function, then
// executes N iterations; the cost of one operation is the callgrind instruction difference
// between a 2N and an N run, divided by N, so start-up, compilation and warm-up cancel.
// Every fixture prints a checksum, which must match Node's in every mode: one table checks
// semantics and cost together.
//
// Modes (Quench): no-jit compiles nothing at runtime (checked: the run maps no executable
// memory of its own); jit runs every runtime compilation layer. aot (compilation before the
// run as well) is a separate build and is not measured here.
// Peers run in their no-JIT modes (Node --jitless, Bun with JSC's JIT off). The budget of an
// operation is the best peer's cost over the win margin, the bar a no-jit Quench row must
// clear; the C column is the work a C programmer's loop does for the same body, the bar of
// the native tiers. Peer costs do not change with Quench builds: --peers caches them.
//
// Usage: tools/op-cost.mjs <quench-node> [--modes no-jit,jit] [--peers file.json]
//        [--baseline file.json] [--save file.json] [ops...]

import { execFileSync, spawn } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

// name, setup (top-level code), the names `run` takes from it, body (per iteration, `i` the
// index, `acc` the accumulator), C instructions per iteration, and optionally the iterations a
// run takes (default N). Setup runs outside the measured function, so the function is the loop
// and its body alone.
const WARM = 3;
const N = 40_000;

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
  [
    "construct",
    "function N(p, s) { this.type = ''; this.start = s; this.end = 0; this.loc = null; }",
    ["N"],
    "var x = new N(null, i); acc = (acc + x.start) | 0;",
    8,
  ],
  // Reads of global bindings: a script's top-level var and function, a built-in global.
  ["global-load", "var g = 5;", [], "acc = (acc + g) | 0;", 4],
  ["global-call", "function gf(x) { return (x + 1) | 0; }", [], "acc = gf(acc);", 5],
  ["builtin-load", "", [], "acc = (acc + Math.PI) | 0;", 4],
  // Calls of built-in functions from JavaScript: a static method, an array method pair, a
  // string method.
  ["native-call", "", [], "acc = (acc + Math.max(i & 7, 3)) | 0;", 6],
  ["push-pop", "var a = [];", ["a"], "a.push(i); acc = (acc + a.pop()) | 0;", 8],
  ["string-method", "var s = 'abcdefgh';", ["s"], "acc = (acc + s.charCodeAt(i & 7)) | 0;", 5],
  // A native's call of a JavaScript function per element: the callback entry and return.
  [
    "array-callback",
    "var cells = [0]; function add(x) { cells[0] = (cells[0] + x) | 0; }",
    ["cells", "add"],
    "[i].forEach(add); acc = cells[0];",
    8,
  ],
  // The same callback over a 64-element array: the per-element cost apart from the entry. Its
  // runs take N calls of the callback, as `call`'s do: callgrind's memory grows with the calls
  // a run makes without leaving the lane, past the host's at N iterations of 64 calls.
  [
    "array-foreach-64",
    "var cells = [0]; function add(x) { cells[0] = (cells[0] + x) | 0; } var a = []; for (var j = 0; j < 64; j++) a.push(j);",
    ["cells", "add", "a"],
    "a.forEach(add); acc = cells[0];",
    64 * 6,
    N / 64,
  ],
];


function fixture(setup, names, body, warm, n) {
  // `acc` is printed as a checksum; doubles keep a fixed rendering through `toFixed`.
  const params = ["n", ...names].join(", ");
  const args = names.map((name) => `, ${name}`).join("");
  return `${setup}
function run(${params}) { var acc = 0; for (var i = 0; i < n; i++) { ${body} } return acc; }
for (var w = 0; w < ${WARM}; w++) run(${warm}${args});
var r = run(${n}${args});
console.log(typeof r === "number" && !Number.isInteger(r) ? r.toFixed(6) : String(r));
`;
}

// The score margin a row must win by: Quench at most the best peer's cost over this.
const WIN_MARGIN = 1.2;

// name, arguments before the fixture, environment.
const MODES = {
  "no-jit": [[], {}],
  jit: [["--jit"], { QUENCH_JS_JIT_LANE_METHODS: "1" }],
};
const BUN = process.env.BUN ?? join(process.env.HOME ?? "", ".bun/bin/bun");
// name, command, arguments, environment.
const PEERS = [
  ["node", "node", ["--jitless"], {}],
  ["bun", BUN, [], { BUN_JSC_useJIT: "0", BUN_OPTIONS: "" }],
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

async function instructions(command, args, env, file) {
  const { err } = await run(
    "valgrind",
    ["--tool=callgrind", "--callgrind-out-file=/dev/null", command, ...args, file],
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
const modeNames = (option("--modes") ?? "no-jit").split(",");
const peersFile = option("--peers");
const baselineFile = option("--baseline");
const saveFile = option("--save");
const modes = modeNames.map((mode) => {
  if (!MODES[mode]) throw new Error(`unknown mode ${mode}: ${Object.keys(MODES).join(", ")}`);
  return [mode, quench, ...MODES[mode]];
});
const chosen = argv.length ? OPERATIONS.filter(([name]) => argv.includes(name)) : OPERATIONS;
const directory = mkdtempSync(join(tmpdir(), "op-cost-"));
const width = Number(execFileSync("nproc").toString().trim());
const cachedPeers = peersFile && existsSync(peersFile) ? JSON.parse(readFileSync(peersFile, "utf8")) : {};

// Whether a no-jit run of `file` writes no code: no executable mapping or protection change
// of its own, only the executable's and its libraries' file mappings.
function writesNoCode(file) {
  const trace = join(directory, "no-jit.strace");
  execFileSync("strace", ["-f", "-e", "trace=mmap,mprotect", "-o", trace, quench, ...MODES["no-jit"][0], file], {
    env: { ...process.env, QUENCH_COMPILE_CACHE: "" },
    stdio: "ignore",
  });
  return !readFileSync(trace, "utf8")
    .split("\n")
    .some((line) => line.includes("PROT_EXEC") && (line.includes("mprotect(") || line.includes("MAP_ANONYMOUS")));
}

const jobs = [];
const cells = {};
const measure = (name, column, command, args, env, single, double, expected, n) =>
  jobs.push(async () => {
    const result = await run(command, [...args, single], env);
    const ok = result.code === 0 && result.out === expected;
    const [a, b] = await Promise.all([
      instructions(command, args, env, single),
      instructions(command, args, env, double),
    ]);
    cells[name][column] = { ok, cost: (b - a) / n, got: result.out };
  });
for (const [name, setup, names, body, , n = N] of chosen) {
  const single = join(directory, `${name}-1.js`);
  const double = join(directory, `${name}-2.js`);
  writeFileSync(single, fixture(setup, names, body, n, n));
  writeFileSync(double, fixture(setup, names, body, n, 2 * n));
  const expected = execFileSync("node", [single]).toString().trim();
  cells[name] = { expected };
  for (const [mode, command, args, env] of modes) measure(name, mode, command, args, env, single, double, expected, n);
  for (const [peer, command, args, env] of PEERS) {
    const cached = cachedPeers[name]?.[peer];
    if (cached) cells[name][peer] = cached;
    else measure(name, peer, command, args, env, single, double, expected, n);
  }
}
const noCode = modeNames.includes("no-jit") ? writesNoCode(join(directory, `${chosen[0][0]}-1.js`)) : true;
await pool(jobs, width);
if (peersFile) {
  const peers = { ...cachedPeers };
  for (const [name] of chosen) peers[name] = Object.fromEntries(PEERS.map(([peer]) => [peer, cells[name][peer]]));
  writeFileSync(peersFile, JSON.stringify(peers, null, 1));
}

const baseline = baselineFile ? JSON.parse(readFileSync(baselineFile, "utf8")) : {};
const budget = (name) => Math.min(...PEERS.map(([peer]) => cells[name][peer]).filter((cell) => cell.ok).map((cell) => cell.cost)) / WIN_MARGIN;
const column = (name, mode) => {
  const cell = cells[name][mode];
  const base = baseline[name]?.[mode]?.cost;
  const value = cell.cost.toFixed(1);
  const delta = base === undefined ? "" : ` (${cell.cost - base >= 0 ? "+" : ""}${(cell.cost - base).toFixed(1)})`;
  return `${cell.ok ? "" : "WRONG "}${value}${delta}`;
};
const columns = [...modes.map(([mode]) => mode), ...PEERS.map(([peer]) => peer)];
console.log(`| operation | C | ${columns.join(" | ")} | budget | ${modeNames[0]}/budget |`);
console.log(`|---|---|${columns.map(() => "---").join("|")}|---|---|`);
for (const [name, , , , c] of chosen) {
  const over = cells[name][modeNames[0]].cost / budget(name);
  console.log(
    `| ${name} | ${c} | ${columns.map((mode) => column(name, mode)).join(" | ")} | ${budget(name).toFixed(1)} | ${over.toFixed(2)}${over > 1 ? " LOSES" : ""} |`,
  );
}
const wrong = chosen.flatMap(([name]) =>
  columns.filter((mode) => !cells[name][mode].ok).map((mode) =>
    `${name}/${mode}: expected ${cells[name].expected}, got ${cells[name][mode].got}`
  )
);
for (const line of wrong) console.log(`WRONG ${line}`);
if (!noCode) console.log("NO-JIT VIOLATED: the no-jit run mapped executable memory of its own");
if (saveFile) writeFileSync(saveFile, JSON.stringify(cells, null, 1));
process.exitCode = wrong.length || !noCode ? 1 : 0;

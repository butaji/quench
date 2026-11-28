// Per-primitive instruction counts: runs one op of ops.js under callgrind as a whole run (no
// profiling marks). The gate runs each op twice, with argv[0] = n and 2n measured iterations, and
// takes (Ir(2n) - Ir(n)) / n: start-up, warm-up and per-call costs cancel, as in
// tools/richards-residue.sh. Each run warms up with the same fixed length, so only the measured
// iterations differ between the two.
// argv: --list | n op. Prints `IRBUDGET <op>` once the op has run, then `IRBUDGET-SINK <value>`.
"use strict";
const fs = require("fs"), path = require("path");
(0, eval)(fs.readFileSync(path.join(__dirname, "ops.js"), "utf8"));
const collect = typeof gc === "function" ? gc : () => {};
const argv = process.argv.slice(2);
// `--list`: the op names, one `IRBUDGET <op>` line each (the gate runs each op in its own process).
if (argv[0] === "--list") { for (const name of Object.keys(PRIMITIVE_OPS)) console.log("IRBUDGET " + name); process.exit(0); }
const WARMUP_N = 20000;
const n = +argv[0] || 20000;
const warmup = +process.env.IRBUDGET_WARMUP || 3;
const name = argv[1];
const op = PRIMITIVE_OPS[name];
if (!op) { console.error("expected: n op (unknown op " + name + ")"); process.exit(2); }
let sink = 0;
// Warm-up: quickening and tier-up settle before the measured run. JIT tiers compile on a later
// entry (IRBUDGET_WARMUP raises the count for them).
for (let r = 0; r < warmup; r++) sink ^= +op(WARMUP_N) | 0;
// The measured run starts from a collected heap (`--expose-gc`), so an allocating op's amortized
// collection cost does not depend on where the previous op left the heap.
collect();
sink ^= +op(n) | 0;
console.log("IRBUDGET " + name);
console.log("IRBUDGET-SINK " + sink);

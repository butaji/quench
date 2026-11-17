// Per-primitive instruction counts: runs each op of ops.js between profiling marks so one
// callgrind run (`--dump-before=quench_ir_mark`, `QUENCH_DEV=ir-mark`) gives every op's
// instructions per iteration as (Ir(2n) - Ir(n)) / n, start-up and per-call costs cancelled.
// argv: n [op ...]. Prints one line per measured op, in mark order: `IRBUDGET <op>`.
// Each op costs three marks: before op(n), between op(n) and op(2n), after op(2n).
"use strict";
const fs = require("fs"), path = require("path");
(0, eval)(fs.readFileSync(path.join(__dirname, "ops.js"), "utf8"));
const mark = process.__irMark;
if (typeof mark !== "function") { console.error("run with QUENCH_DEV=ir-mark"); process.exit(2); }
const argv = process.argv.slice(2);
const n = +argv[0] || 20000;
const names = argv.length > 1 ? argv.slice(1) : Object.keys(PRIMITIVE_OPS);
let sink = 0;
for (const name of names) {
  const op = PRIMITIVE_OPS[name];
  if (!op) { console.error("unknown op " + name); process.exit(2); }
  // Warm-up: quickening and tier-up settle before the measured runs.
  for (let r = 0; r < 3; r++) sink ^= +op(n) | 0;
  mark();
  sink ^= +op(n) | 0;
  mark();
  sink ^= +op(2 * n) | 0;
  mark();
  console.log("IRBUDGET " + name);
}
console.log("IRBUDGET-SINK " + sink);

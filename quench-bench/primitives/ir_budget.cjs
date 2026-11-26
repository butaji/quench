// Per-primitive instruction counts: runs each op of ops.js between profiling marks so one
// callgrind run (`--dump-before=quench_ir_mark`, `QUENCH_DEV=ir-mark`) gives every op's
// instructions per iteration as (Ir(2n) - Ir(n)) / n, start-up and per-call costs cancelled.
// argv: n [op ...]. Prints one line per measured op, in mark order: `IRBUDGET <op>`.
// Each op costs four marks per pair: around op(n), then around op(2n), each after a collection.
"use strict";
const fs = require("fs"), path = require("path");
(0, eval)(fs.readFileSync(path.join(__dirname, "ops.js"), "utf8"));
const mark = process.__irMark;
const collect = typeof gc === "function" ? gc : () => {};
if (typeof mark !== "function") { console.error("run with QUENCH_DEV=ir-mark"); process.exit(2); }
const argv = process.argv.slice(2);
const n = +argv[0] || 20000;
const warmup = +process.env.IRBUDGET_WARMUP || 3;
const pairs = +process.env.IRBUDGET_PAIRS || 1;
const names = argv.length > 1 ? argv.slice(1) : Object.keys(PRIMITIVE_OPS);
let sink = 0;
for (const name of names) {
  const op = PRIMITIVE_OPS[name];
  if (!op) { console.error("unknown op " + name); process.exit(2); }
  // Warm-up: quickening and tier-up settle before the measured runs.
  // JIT tiers compile on a later entry (IRBUDGET_WARMUP raises the count for them).
  for (let r = 0; r < warmup; r++) sink ^= +op(n) | 0;
  // Each measured run starts from the same heap state: collected (`--expose-gc`), then one
  // unmeasured run, which absorbs the collection's deferred sweeping. An allocating op's
  // amortized collection cost then does not depend on where the previous op left the heap.
  // IRBUDGET_PAIRS=2 (the JIT tiers) measures the pair twice: a compile that lands inside one
  // window (promotion at a doubling of completions, a retire and recompile) inflates that window
  // only, and the gate takes the smaller of each length's windows.
  for (let p = 0; p < pairs; p++) {
    collect();
    sink ^= +op(n) | 0;
    mark();
    sink ^= +op(n) | 0;
    mark();
    collect();
    sink ^= +op(n) | 0;
    mark();
    sink ^= +op(2 * n) | 0;
    mark();
  }
  console.log("IRBUDGET " + name);
}
console.log("IRBUDGET-SINK " + sink);

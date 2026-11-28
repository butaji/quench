// Warms every op of ops.js as the ir-budget tier runs do (IRBUDGET_WARMUP, default 12), naming
// each op on stderr first, so a jit-dump trace (QUENCH_TRACE=jit-dump=DIR)
// files the code each op compiled under the op's name.
"use strict";
(0, eval)(require("fs").readFileSync(process.argv[2], "utf8"));
const warmup = +process.env.IRBUDGET_WARMUP || 12;
let sink = 0;
for (const name of Object.keys(PRIMITIVE_OPS)) {
  console.error("IRPREDICT-OP " + name);
  for (let r = 0; r < warmup; r++) sink ^= +PRIMITIVE_OPS[name](2000) | 0;
}
console.log(sink);

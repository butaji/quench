// Runs every op of ops.js as the ir-budget warm-up does, so a lane-plan trace also shows the
// handler each record settled on.
"use strict";
(0, eval)(require("fs").readFileSync(process.argv[2], "utf8"));
let sink = 0;
for (const name of Object.keys(PRIMITIVE_OPS)) for (let r = 0; r < 5; r++) sink ^= +PRIMITIVE_OPS[name](2000) | 0;
console.log(sink);

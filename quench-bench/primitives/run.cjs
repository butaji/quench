// Runs one op (argv: name n reps) and prints {"op":..,"ns":median ns/iteration over reps}.
"use strict";
const argv = typeof process !== "undefined" ? process.argv.slice(2) : [];
const name = argv[0], n = +argv[1] || 5000000, reps = +argv[2] || 5, inner = +argv[3] || 0; // inner>0: n total iterations as n/inner calls of op(inner)
const fs = require("fs"), path = require("path");
(0, eval)(fs.readFileSync(path.join(__dirname, "ops.js"), "utf8"));
const f = PRIMITIVE_OPS[name];
if (!f) { console.error("unknown op " + name); process.exit(2); }
const times = []; let sink = 0;
f(1000); // short warm-up; tier-up policy under test must do the rest
for (let r = 0; r < reps; r++) {
  const t0 = process.hrtime.bigint();
  let v = 0;
  if (inner > 0) { for (let c = n / inner; c > 0; c--) v = (v + f(inner)) | 0; } else v = f(n);
  const t1 = process.hrtime.bigint();
  sink = (sink + (typeof v === "number" ? v : 0)) | 0;
  times.push(Number(t1 - t0) / n);
}
times.sort((a, b) => a - b);
console.log(JSON.stringify({ op: name, ns: +times[times.length >> 1].toFixed(3), min: +times[0].toFixed(3), sink }));

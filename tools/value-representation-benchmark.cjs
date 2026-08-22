#!/usr/bin/env node
"use strict";
const iterations = Number(process.env.QUENCH_VALUE_ITERATIONS || 1_000_000);
const tagged = new Array(iterations);
const boxed = new Float64Array(iterations);
for (let i = 0; i < iterations; i++) {
  tagged[i] = { tag: i & 1, payload: i };
  boxed[i] = i;
}
function measure(name, read) {
  let checksum = 0;
  const started = process.hrtime.bigint();
  for (let i = 0; i < iterations; i++) checksum += read(i);
  return { representation: name, iterations, checksum, wall_ms: Number(process.hrtime.bigint() - started) / 1e6 };
}
const results = [
  measure("tagged", (i) => tagged[i].payload),
  measure("boxed-payload", (i) => boxed[i]),
];
if (results[0].checksum !== results[1].checksum) throw new Error("representation checksum mismatch");
console.log(JSON.stringify({ results, boxed_bytes: boxed.byteLength }));

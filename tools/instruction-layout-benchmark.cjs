#!/usr/bin/env node
"use strict";
const iterations = Number(process.env.QUENCH_LAYOUT_ITERATIONS || 1_000_000);
const fixed = new Uint16Array(iterations * 4);
for (let i = 0; i < iterations; i++) {
  fixed[i * 4] = i & 7;
  fixed[i * 4 + 1] = i & 0xffff;
}
const variable = Array.from({ length: iterations }, (_, i) => [i & 7, i & 0xffff, (i + 1) & 0xffff]);
function measure(name, read) {
  let checksum = 0;
  const started = process.hrtime.bigint();
  for (let i = 0; i < iterations; i++) checksum += read(i);
  return { layout: name, iterations, checksum, wall_ms: Number(process.hrtime.bigint() - started) / 1e6 };
}
const results = [
  measure("fixed-width", (i) => fixed[i * 4] + fixed[i * 4 + 1]),
  measure("variable-width", (i) => variable[i][0] + variable[i][1]),
];
if (results.some(({ checksum }) => checksum !== results[0].checksum)) throw new Error("layout checksum mismatch");
console.log(JSON.stringify({ results, fixed_bytes: fixed.byteLength }));

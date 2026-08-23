#!/usr/bin/env node
"use strict";
const iterations = Number(process.env.QUENCH_VALUE_ITERATIONS || 1_000_000);
if (!Number.isSafeInteger(iterations) || iterations < 1) {
  throw new Error("QUENCH_VALUE_ITERATIONS must be a positive safe integer");
}
// The record is the canonical fact under test: tag selects the representation
// and payload owns the numeric value. Both measurements consume these facts;
// no second semantic model is introduced.
const tagged = new Array(iterations);
const boxed = new Float64Array(iterations);
for (let i = 0; i < iterations; i++) {
  tagged[i] = { tag: i & 1, payload: i };
  boxed[i] = i;
}
function dispatchTagged(record) {
  if (record.tag !== 0 && record.tag !== 1) throw new Error("invalid value tag");
  return record.payload;
}
function measure(name, read) {
  let checksum = 0;
  const started = process.hrtime.bigint();
  for (let i = 0; i < iterations; i++) checksum += read(i);
  return { representation: name, iterations, checksum, wall_ms: Number(process.hrtime.bigint() - started) / 1e6 };
}
const results = [
  measure("tagged-payload", (i) => tagged[i].payload),
  measure("tagged-dispatch", (i) => dispatchTagged(tagged[i])),
  measure("boxed-payload", (i) => boxed[i]),
];
const expectedChecksum = (iterations * (iterations - 1)) / 2;
if (results.some(({ checksum }) => checksum !== expectedChecksum)) {
  throw new Error("representation checksum mismatch");
}
console.log(JSON.stringify({ results, boxed_bytes: boxed.byteLength }));

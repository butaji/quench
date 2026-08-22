#!/usr/bin/env node
"use strict";
const iterations = Number(process.env.QUENCH_PERF_ITERATIONS || 100000);
let checksum = 0;
let allocations = 0;
let copies = 0;
let bytes = 0;
let peakRss = process.memoryUsage().rss;
const sampleRss = () => {
  const rss = process.memoryUsage().rss;
  if (rss > peakRss) peakRss = rss;
};
const started = process.hrtime.bigint();
for (let i = 0; i < iterations; i++) {
  const kind = i & 3;
  const text = `value-${i & 255}`;
  const value = { kind, text, number: i };
  allocations++;
  checksum += value.kind + value.text.length + value.number;
  copies += text.length;
  bytes += text.length + 16;
  if ((i & 1023) === 0) sampleRss();
}
const wallMs = Number(process.hrtime.bigint() - started) / 1e6;
if (checksum <= 0 || !Number.isFinite(checksum)) throw new Error("workload checksum mismatch");
console.log(JSON.stringify({ iterations, checksum, allocations, copies, bytes, peak_rss: peakRss, wall_ms: wallMs }));

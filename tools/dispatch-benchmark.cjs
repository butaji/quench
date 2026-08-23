#!/usr/bin/env node
"use strict";

const iterations = Number(process.env.QUENCH_DISPATCH_ITERATIONS || 1_000_000);
if (!Number.isSafeInteger(iterations) || iterations < 1) {
  throw new Error("QUENCH_DISPATCH_ITERATIONS must be a positive safe integer");
}
const opcodes = new Uint8Array(iterations);

function match(op) {
  switch (op) {
    case 0: return 3;
    case 1: return 5;
    case 2: return 7;
    case 3: return 11;
    case 4: return 13;
    case 5: return 17;
    case 6: return 19;
    default: return 23;
  }
}
const table = [3, 5, 7, 11, 13, 17, 19, 23];
function tableDispatch(op) { return table[op]; }

function measure(name, dispatch) {
  let checksum = 0;
  const started = process.hrtime.bigint();
  for (let i = 0; i < iterations; i++) checksum += dispatch(opcodes[i]);
  const wallMs = Number(process.hrtime.bigint() - started) / 1e6;
  return { strategy: name, iterations, checksum, wall_ms: wallMs };
}

const results = [measure("match", match), measure("table", tableDispatch)];
const expectedChecksum = Array.from(opcodes, (opcode) => table[opcode]).reduce((sum, value) => sum + value, 0);
if (results.some((result) => result.checksum !== expectedChecksum)) {
  throw new Error("dispatch checksum mismatch");
}
console.log(JSON.stringify({ results }));

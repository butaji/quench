#!/usr/bin/env node
'use strict';
// Node-side reference for the C analogue of JetStream's mandreel row (mandreel.c).
//
// usage: node tools/mandreel-validate.cjs [--iterations N] [--jetstream DIR] [--json FILE]
//                                         [--write-c mandreel.c] [--time]
//
// Runs the row's program under node 22 the way the JetStream driver runs it (the mandreel.js
// file, then `new Benchmark(...).runIteration(i)` for each iteration), and hashes the state
// that the C analogue hashes:
//   - the heap: FNV-1a over the 32-bit words of heap32, in address order,
//   - the numeric JS globals the translator lists (names sorted; canonical NaN for undefined and
//     non-numbers), as FNV-1a over the two 32-bit halves of each value's bit pattern.
// The hashes are taken after the file has loaded (the setup) and after each iteration.
// --write-c rewrites the expected-values block of mandreel.c. --time runs the iterations without
// hashing and prints the node time of the iterations, for comparison with the C time.
const fs = require('fs');
const path = require('path');
const vm = require('vm');

const here = __dirname;
const { emitFile, UNHASHED_GLOBALS } = require(path.join(here, 'mandreel-to-c.cjs'));

function arg(name, fallback) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : fallback;
}
const iterations = Number(arg('--iterations', '80'));
const jetstream = arg('--jetstream', '/home/user/quench/quench-bench/jetstream');
const jsonOut = arg('--json', null);
const writeC = arg('--write-c', null);
const dumpDir = arg('--dump', null); // setup state: heap as raw int32 words, globals as name and bits
const timeOnly = process.argv.includes('--time');

// The state names, in the translator's order (the C table is the same list).
const src = fs.readFileSync(path.join(jetstream, 'Octane', 'mandreel.js'), 'utf8');
const { prog } = emitFile(src);
const names = [...prog.globals].sort().filter((n) => !prog.fnvars.has(n) && !prog.stringVars.has(n) && !UNHASHED_GLOBALS.has(n));

// The Benchmark class calls runMandreel; the hashing and the timing are added around the loop
// by the harness below, not inside mandreel.js.
const prelude = `
globalThis.print = globalThis.print ?? console.log;
globalThis.self = globalThis;
`;
const harness = `
;(function () {
  const names = ${JSON.stringify(names)};
  const f64 = new Float64Array(1);
  const u32 = new Uint32Array(f64.buffer);
  const CANON_HI = 0x7ff80000, CANON_LO = 0;
  // Numbers hash as themselves, booleans as 0/1 (the C keeps them as doubles). Other values (objects,
  // functions, undefined) hash as canonical NaN: the C has no value for them, except the few
  // marker globals that are left out of the names list (UNHASHED_GLOBALS).
  function numericValue(v) {
    if (typeof v === 'number') return v;
    if (typeof v === 'boolean') return v ? 1 : 0;
    return NaN;
  }
  function fnvWord(h, w) { return Math.imul((h ^ w) >>> 0, 16777619) >>> 0; }
  function heapHash() {
    let h = 0x811c9dc5;
    const words = heap32;
    for (let i = 0; i < words.length; i++) h = fnvWord(h, words[i] >>> 0);
    return h >>> 0;
  }
  function globalsHash() {
    let h = 0x811c9dc5;
    for (const name of names) {
      const v = numericValue(globalThis[name]);
      if (typeof v === 'number' && v === v) { f64[0] = v; h = fnvWord(h, u32[0]); h = fnvWord(h, u32[1]); }
      else { h = fnvWord(h, CANON_LO); h = fnvWord(h, CANON_HI); }
    }
    return h >>> 0;
  }
  const out = { setupHeap: null, setupGlobals: null, heap: [], globals: [], iterMs: 0 };
  const benchmark = new Benchmark({ iterationCount: ${iterations} });
  const hashes = ${timeOnly ? 'false' : 'true'};
  if (hashes) { out.setupHeap = heapHash(); out.setupGlobals = globalsHash(); }
  if (${dumpDir ? 'true' : 'false'}) {
    const lines = [];
    for (const name of names) {
      const num = numericValue(globalThis[name]);
      f64[0] = num === num ? num : NaN;
      lines.push(name + ' ' + (num === num ? num : 'NaN') + ' ' + u32[1].toString(16) + ' ' + u32[0].toString(16));
    }
    require('fs').writeFileSync(${JSON.stringify(dumpDir)} + '/node-globals.txt', lines.join('\\n') + '\\n');
    require('fs').writeFileSync(${JSON.stringify(dumpDir)} + '/node-heap.bin', Buffer.from(heap32.buffer, heap32.byteOffset, heap32.byteLength));
  }
  let total = 0;
  for (let i = 0; i < ${iterations}; i++) {
    const t0 = performance.now();
    benchmark.runIteration(i);
    total += performance.now() - t0;
    if (hashes) { out.heap.push(heapHash()); out.globals.push(globalsHash()); }
  }
  out.iterMs = total;
  globalThis.__validateOut = out;
})();
`;
const program = prelude + src + '\n' + harness;

globalThis.require = require;
const runner = new vm.Script(program, { filename: 'mandreel-validate.js' });
runner.runInThisContext();
const out = globalThis.__validateOut;

const hex = (n) => '0x' + (n >>> 0).toString(16).padStart(8, '0') + 'u';
console.log(`node ${iterations} iterations ${out.iterMs.toFixed(3)} ms`);
if (!timeOnly) {
  console.log(`setup heap ${hex(out.setupHeap)} globals ${hex(out.setupGlobals)}; ${names.length} numeric globals`);
  console.log(`first iteration heap ${hex(out.heap[0])} globals ${hex(out.globals[0])}`);
  console.log(`last iteration heap ${hex(out.heap[iterations - 1])} globals ${hex(out.globals[iterations - 1])}`);
}
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(out));

if (writeC) {
  const expected = [
    '/* BEGIN EXPECTED (generated by tools/mandreel-validate.cjs --write-c; do not edit) */',
    '#define HAVE_EXPECTED 1',
    `#define EXPECT_ITERATIONS ${iterations}`,
    `static const uint32_t expect_setup_heap = ${hex(out.setupHeap)};`,
    `static const uint32_t expect_setup_globals = ${hex(out.setupGlobals)};`,
    `static const uint32_t expect_heap[EXPECT_ITERATIONS] = {${out.heap.map(hex).join(', ')}};`,
    `static const uint32_t expect_globals[EXPECT_ITERATIONS] = {${out.globals.map(hex).join(', ')}};`,
    '/* END EXPECTED */',
  ].join('\n');
  const text = fs.readFileSync(writeC, 'utf8');
  const begin = text.indexOf('/* BEGIN EXPECTED');
  const end = text.indexOf('/* END EXPECTED */');
  if (begin < 0 || end < 0) throw new Error('markers not found in ' + writeC);
  fs.writeFileSync(writeC, text.slice(0, begin) + expected + text.slice(end + '/* END EXPECTED */'.length));
  console.log('wrote expected values to ' + writeC);
}

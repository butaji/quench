#!/usr/bin/env node
'use strict';

const fs = require('node:fs');
const os = require('node:os');
const { spawnSync } = require('node:child_process');
const vm = require('node:vm');
const { Worker, isMainThread, parentPort, workerData } = require('node:worker_threads');

if (!isMainThread) {
  const { sab, index, startIndex, iters } = workerData;
  const view = new Int32Array(sab);
  parentPort.postMessage('ready');
  Atomics.wait(view, startIndex, 0);
  for (let i = 0; i < iters; i++) Atomics.add(view, index, 1);
  parentPort.postMessage('done');
  return;
}

const ITERATIONS = positiveInt(process.env.BENCH_ITERATIONS, 1_000_000);
const CACHE_ITERS = Math.min(250_000, positiveInt(process.env.CACHE_ITERS, 80_000));
const MAX_WALL_MS = positiveInt(process.env.MAX_WALL_MS, 30_000);
const MAX_TOTAL_MS = positiveInt(process.env.MAX_TOTAL_MS, 55_000);
// Optional regression gate. Keep it disabled by default because timings are
// machine-specific; CI or a local pinned machine can provide a budget.
const MAX_PER_OP_NS = positiveInt(process.env.MAX_PER_OP_NS, 0);
const budgetEnabled = process.env.MAX_PER_OP_NS !== undefined && MAX_PER_OP_NS > 0;
const budgetFailures = [];
// Keep repeats deliberately small so the harness remains a bounded probe.
const REPEATS = Math.min(5, positiveInt(process.env.BENCH_REPEATS, 3));
const CACHE_REPEATS = Math.min(REPEATS, 2);
const workloads = [
  ['arithmetic', `let x = 0; for (let i = 0; i < ITERATIONS; i++) x = (x + i) * 1.000001; x;`],
  ['property-read-write', `const o = { value: 0 }; for (let i = 0; i < ITERATIONS; i++) o.value = o.value + 1; o.value;`],
  ['dense-array-read-write', `const a = new Array(1024).fill(0); for (let i = 0; i < ITERATIONS; i++) { const j = i & 1023; a[j] = a[j] + 1; } a[0];`],
  ['function-call', `function f(x) { return (x + 1) | 0; } let x = 0; for (let i = 0; i < ITERATIONS; i++) x = f(x); x;`],
  ['regex-match', `const re = /^[a-z]+-[0-9]+$/; let matches = 0; for (let i = 0; i < ITERATIONS; i++) if (re.test('item-12345')) matches++; matches;`],
];

function positiveInt(value, fallback) {
  const n = Number(value);
  return Number.isSafeInteger(n) && n > 0 ? n : fallback;
}
function now() { return process.hrtime.bigint(); }
function memory() { const m = process.memoryUsage(); return { rss: m.rss, heapUsed: m.heapUsed }; }
function collect() { if (typeof global.gc === 'function') global.gc(); }
function percentile(values, p) {
  const sorted = values.slice().sort((a, b) => a < b ? -1 : a > b ? 1 : 0);
  return sorted[Math.max(0, Math.ceil(sorted.length * p) - 1)];
}
function result(name, samples, iterations) {
  const walls = samples.map((sample) => sample.wallNs);
  const median = percentile(walls, 0.5);
  const p95 = percentile(walls, 0.95);
  const representative = samples.find((sample) => sample.wallNs === median) || samples[0];
  const rss = Math.round(samples.reduce((sum, sample) => sum + sample.rssDelta, 0) / samples.length);
  const heapDelta = Math.round(samples.reduce((sum, sample) => sum + sample.heapDelta, 0) / samples.length);
  const iters = BigInt(iterations);
  return {
    workload: name, iterations, wall_ns: median.toString(),
    per_op_ns: (median / iters).toString(), rss_delta_bytes: rss,
    allocs_proxy: Math.round(heapDelta / 64), timed_out: samples.some((sample) => sample.timedOut),
    repeat_count: samples.length, wall_ns_median: median.toString(),
    wall_ns_p95: p95.toString(), per_op_ns_median: (median / iters).toString(),
    per_op_ns_p95: (p95 / iters).toString(),
    timed_out_repeats: samples.filter((sample) => sample.timedOut).length,
    representative_rss_delta_bytes: representative.rssDelta,
  };
}

function sysctlNumber(name) {
  const ran = spawnSync('sysctl', ['-n', name], { encoding: 'utf8', timeout: 1000 });
  if (ran.status !== 0) return null;
  const n = Number(String(ran.stdout).trim());
  return Number.isFinite(n) && n > 0 ? n : null;
}

function readNumber(path) {
  try {
    const n = Number(fs.readFileSync(path, 'utf8').trim());
    return Number.isFinite(n) && n > 0 ? n : null;
  } catch {
    return null;
  }
}

function measureCacheLine() {
  if (process.platform === 'darwin') {
    const bytes = sysctlNumber('hw.cachelinesize');
    if (bytes) return { bytes, source: 'sysctl hw.cachelinesize' };
  }
  const linux = readNumber('/sys/devices/system/cpu/cpu0/cache/index0/coherency_line_size');
  if (linux) return { bytes: linux, source: '/sys/devices/system/cpu/cpu0/cache/index0/coherency_line_size' };
  return { bytes: 64, source: 'assumed-64-not-measured' };
}

function measurePageBytes() {
  if (process.platform === 'darwin') {
    const bytes = sysctlNumber('hw.pagesize');
    if (bytes) return { bytes, source: 'sysctl hw.pagesize' };
  }
  const ran = spawnSync('getconf', ['PAGE_SIZE'], { encoding: 'utf8', timeout: 1000 });
  if (ran.status === 0) {
    const n = Number(String(ran.stdout).trim());
    if (Number.isFinite(n) && n > 0) return { bytes: n, source: 'getconf PAGE_SIZE' };
  }
  return { bytes: 4096, source: 'assumed-4096-not-measured' };
}

function machineRecord() {
  const line = measureCacheLine();
  const page = measurePageBytes();
  const l1d = process.platform === 'darwin' ? sysctlNumber('hw.l1dcachesize') : null;
  const l1i = process.platform === 'darwin' ? sysctlNumber('hw.l1icachesize') : null;
  const l2 = process.platform === 'darwin' ? sysctlNumber('hw.l2cachesize') : null;
  const stride = Math.max(64, line.bytes);
  return {
    arch: os.arch(),
    platform: process.platform,
    cache_line_bytes: line.bytes,
    cache_line_source: line.source,
    page_bytes: page.bytes,
    page_source: page.source,
    l1d_bytes: l1d,
    l1i_bytes: l1i,
    l2_bytes: l2,
    portable_header_budget_bytes: 64,
    false_share_stride_bytes: stride,
    prefetch_policy: 'forbidden-until-profiled',
    layout_measured_at: '2026-08-22',
    layout_target: 'aarch64-apple-darwin rustc 1.97.1',
    layouts: {
      Value: { size: 32, align: 8, variants: 34 },
      TaggedValue: { size: 8, align: 8 },
      ObjectData: { size: 192, align: 8 },
      ArrayData: { size: 168, align: 8 },
      ArrayBufferData: { size: 120, align: 8 },
      FunctionValue: { size: 104, align: 8 },
      BoundFunctionValue: { size: 128, align: 8 },
      PromiseData: { size: 232, align: 8 },
      Op: { size: 152, align: 8, variants: 102 },
      HotOp: { size: 1, align: 1 },
      Machine: { size: 192, align: 8 },
      Frame: { size: 80, align: 8 },
      Completion: { size: 88, align: 8 },
      PackedCompletion: { size: 12, align: 4 },
      HeapRef: { size: 4, align: 4, note: 'identity::HeapRef' },
      TaggedHeapRef: { size: 8, align: 4, note: 'tagged_value::HeapRef' },
      ShapeCache4: { size: 72, align: 8 },
      ShapeCache4Cell: { size: 80, align: 8, note: 'RefCell<ShapeCache<4>>' },
      Activation_derived: { size: 104, align: 8, note: 'field-sum; not size_of' },
    },
    occupancy: {
      portable_line_bytes: 64,
      values_per_line: 2,
      tagged_per_line: 8,
      ops_per_line: Number((64 / 152).toFixed(4)),
      object_headers_per_line: Number((64 / 192).toFixed(4)),
    },
    decisions: {
      object_header: 'aos-compact-header-plus-value-slots',
      shape_ic: 'side-table-or-monomorphic-inline',
      numeric_arrays: 'soa-packed-kind',
      ops: 'soa-opcode-stream',
      registers_frames: 'aos-whole-record',
      isolate_counters: 'padded-soa',
      prefetch: 'forbidden-until-profiled',
      atomics: 'guest-sab-only',
    },
  };
}

function sampleNative(fn, iterations, started) {
  const remaining = MAX_TOTAL_MS - (Date.now() - started);
  const maxIterations = Math.min(2_000_000, Math.max(0, MAX_WALL_MS * 100_000));
  if (remaining <= 0 || iterations > maxIterations) {
    return { wallNs: 0n, rssDelta: 0, heapDelta: 0, timedOut: true };
  }
  collect();
  const before = memory();
  const begin = now();
  let timedOut = false;
  try {
    fn(iterations);
  } catch (error) {
    timedOut = Boolean(error && error.code === 'ERR_SCRIPT_EXECUTION_TIMEOUT');
    if (!timedOut) throw error;
  }
  const wallNs = now() - begin;
  collect();
  const after = memory();
  return {
    wallNs,
    rssDelta: after.rss - before.rss,
    heapDelta: Math.max(0, after.heapUsed - before.heapUsed),
    timedOut,
  };
}

function prepareAos() {
  const n = 4096;
  const rows = new Array(n);
  for (let i = 0; i < n; i++) rows[i] = { x: i, y: 1, z: 0 };
  return rows;
}
function aosScan(rows, iterations) {
  const n = rows.length;
  let sum = 0;
  for (let i = 0; i < iterations; i++) sum += rows[i & (n - 1)].x;
  return sum;
}

function prepareSoa() {
  const n = 4096;
  const xs = new Float64Array(n);
  const ys = new Float64Array(n);
  for (let i = 0; i < n; i++) { xs[i] = i; ys[i] = 1; }
  return { xs, ys };
}
function soaScan(data, iterations) {
  const xs = data.xs;
  const n = xs.length;
  let sum = 0;
  for (let i = 0; i < iterations; i++) sum += xs[i & (n - 1)];
  return sum;
}

function prepareSequential() {
  const n = 65536;
  const values = new Float64Array(n);
  for (let i = 0; i < n; i++) values[i] = i;
  return values;
}
function sequentialF64(values, iterations) {
  const n = values.length;
  let sum = 0;
  for (let i = 0; i < iterations; i++) sum += values[i & (n - 1)];
  return sum;
}

function prepareRandom() {
  const n = 65536;
  const values = new Float64Array(n);
  const index = new Int32Array(n);
  let seed = 1;
  for (let i = 0; i < n; i++) {
    values[i] = i;
    seed = (seed * 1664525 + 1013904223) >>> 0;
    index[i] = seed & (n - 1);
  }
  return { values, index };
}
function randomF64(data, iterations) {
  const values = data.values;
  const index = data.index;
  const n = values.length;
  let sum = 0;
  for (let i = 0; i < iterations; i++) sum += values[index[i & (n - 1)]];
  return sum;
}

function preparePointer() {
  const n = 4096;
  const nodes = new Array(n);
  for (let i = 0; i < n; i++) nodes[i] = { value: i, next: null };
  for (let i = 0; i < n; i++) nodes[i].next = nodes[(i + 1) % n];
  return nodes[0];
}
function pointerChase(head, iterations) {
  let node = head;
  let sum = 0;
  for (let i = 0; i < iterations; i++) {
    sum += node.value;
    node = node.next;
  }
  return sum;
}

function prepareIndex() {
  const n = 4096;
  const values = new Float64Array(n);
  const next = new Int32Array(n);
  for (let i = 0; i < n; i++) {
    values[i] = i;
    next[i] = (i + 1) & (n - 1);
  }
  return { values, next };
}
function indexChase(data, iterations) {
  let cursor = 0;
  let sum = 0;
  const values = data.values;
  const next = data.next;
  for (let i = 0; i < iterations; i++) {
    sum += values[cursor];
    cursor = next[cursor];
  }
  return sum;
}

function prepareWholeAos() {
  const n = 4096;
  const rows = new Array(n);
  for (let i = 0; i < n; i++) rows[i] = { x: i, y: 1, z: 2 };
  return rows;
}
function wholeAosScan(rows, iterations) {
  const n = rows.length;
  let sum = 0;
  for (let i = 0; i < iterations; i++) {
    const row = rows[i & (n - 1)];
    sum += row.x + row.y + row.z;
  }
  return sum;
}

function prepareWholeSoa() {
  const n = 4096;
  const xs = new Float64Array(n);
  const ys = new Float64Array(n);
  const zs = new Float64Array(n);
  for (let i = 0; i < n; i++) {
    xs[i] = i;
    ys[i] = 1;
    zs[i] = 2;
  }
  return { xs, ys, zs };
}
function wholeSoaScan(data, iterations) {
  const n = data.xs.length;
  let sum = 0;
  for (let i = 0; i < iterations; i++) {
    const j = i & (n - 1);
    sum += data.xs[j] + data.ys[j] + data.zs[j];
  }
  return sum;
}

function prepareValue32() {
  const n = 4096;
  const words = 4;
  const rows = new Float64Array(n * words);
  for (let i = 0; i < n; i++) rows[i * words] = i;
  return { rows, words, mask: n - 1 };
}
function value32Scan(data, iterations) {
  const rows = data.rows;
  const words = data.words;
  const mask = data.mask;
  let sum = 0;
  for (let i = 0; i < iterations; i++) sum += rows[(i & mask) * words];
  return sum;
}

function preparePackedF64() {
  const n = 4096;
  const values = new Float64Array(n);
  for (let i = 0; i < n; i++) values[i] = i;
  return values;
}
function packedF64Scan(values, iterations) {
  const n = values.length;
  let sum = 0;
  for (let i = 0; i < iterations; i++) sum += values[i & (n - 1)];
  return sum;
}

function prepareHeader192() {
  const n = 2048;
  const words = 192 / 8;
  const rows = new Float64Array(n * words);
  for (let i = 0; i < n; i++) {
    rows[i * words] = i;
    rows[i * words + words - 1] = i + 1;
  }
  return { rows, words, mask: n - 1 };
}
function headerHotPrefix(data, iterations) {
  const rows = data.rows;
  const words = data.words;
  const mask = data.mask;
  let sum = 0;
  for (let i = 0; i < iterations; i++) sum += rows[(i & mask) * words];
  return sum;
}
function headerColdTail(data, iterations) {
  const rows = data.rows;
  const words = data.words;
  const mask = data.mask;
  let sum = 0;
  for (let i = 0; i < iterations; i++) sum += rows[(i & mask) * words + words - 1];
  return sum;
}

function spawnCounter(sab, index, startIndex, iters) {
  return new Promise((resolve, reject) => {
    const worker = new Worker(__filename, { workerData: { sab, index, startIndex, iters } });
    let ready = false;
    worker.on('message', (message) => {
      if (message === 'ready') {
        ready = true;
        resolve({ worker, ready: true });
      } else if (message === 'done') {
        worker.done = true;
        if (worker.notifyDone) worker.notifyDone();
      }
    });
    worker.once('error', reject);
    worker.once('exit', (code) => {
      if (code !== 0 && !ready) reject(new Error(`worker exit ${code}`));
    });
  });
}

async function falseShare(strideInts, iterations, started) {
  const left = 0;
  const right = strideInts;
  const startIndex = strideInts * 2 + 2;
  const sab = new SharedArrayBuffer((startIndex + 1) * 4);
  const view = new Int32Array(sab);
  const deadline = Math.min(MAX_WALL_MS, MAX_TOTAL_MS - (Date.now() - started));
  if (deadline <= 0) return { wallNs: 0n, timedOut: true };
  let workers;
  try {
    workers = await Promise.race([
      Promise.all([
        spawnCounter(sab, left, startIndex, iterations),
        spawnCounter(sab, right, startIndex, iterations),
      ]),
      new Promise((_, reject) => setTimeout(() => reject(new Error('false-share startup timeout')), deadline)),
    ]);
    const finished = Promise.all(workers.map(({ worker }) => new Promise((resolve) => {
      if (worker.done) return resolve();
      worker.notifyDone = resolve;
    })));
    const begin = now();
    Atomics.store(view, startIndex, 1);
    Atomics.notify(view, startIndex, 2);
    await Promise.race([
      finished,
      new Promise((_, reject) => setTimeout(() => reject(new Error('false-share timeout')), deadline)),
    ]);
    return { wallNs: now() - begin, timedOut: false };
  } finally {
    if (workers) await Promise.all(workers.map(({ worker }) => worker.terminate()));
  }
}

function runTimed(name, prepare, scan, iterations, repeats, started) {
  const payload = prepare();
  const samples = [];
  while (samples.length < repeats && Date.now() - started < MAX_TOTAL_MS) {
    samples.push(sampleNative(() => scan(payload, iterations), iterations, started));
  }
  return samples.length ? result(name, samples, iterations) : null;
}

function runTimed(name, prepare, scan, iterations, repeats, started) {
  const payload = prepare();
  const samples = [];
  while (samples.length < repeats && Date.now() - started < MAX_TOTAL_MS) {
    samples.push(sampleNative(() => scan(payload, iterations), iterations));
  }
  return samples.length ? result(name, samples, iterations) : null;
}

async function main() {
  const started = Date.now();
  const output = [];
  for (const [name, source] of workloads) {
    const script = new vm.Script(`(function(){ const ITERATIONS = ${ITERATIONS}; ${source}\n})()`);
    const samples = [];
    while (samples.length < REPEATS && Date.now() - started < MAX_TOTAL_MS) {
      const remaining = MAX_TOTAL_MS - (Date.now() - started);
      const budgetMs = Math.max(1, Math.min(MAX_WALL_MS, remaining));
      collect();
      const before = memory();
      const begin = now();
      let timedOut = false;
      try {
        script.runInNewContext(Object.create(null), { timeout: budgetMs });
      } catch (error) {
        timedOut = error && (error.code === 'ERR_SCRIPT_EXECUTION_TIMEOUT' || /timed out/i.test(String(error.message)));
        if (!timedOut) throw error;
      }
      const wallNs = now() - begin;
      collect();
      const after = memory();
      samples.push({ wallNs, rssDelta: after.rss - before.rss, heapDelta: Math.max(0, after.heapUsed - before.heapUsed), timedOut });
    }
    if (samples.length) {
      const row = result(name, samples, ITERATIONS);
      output.push(row);
      if (budgetEnabled && Number(row.per_op_ns_p95) > MAX_PER_OP_NS) {
        budgetFailures.push({
          workload: name,
          per_op_ns_p95: Number(row.per_op_ns_p95),
          max_per_op_ns: MAX_PER_OP_NS,
        });
      }
    }
    if (Date.now() - started >= MAX_TOTAL_MS) break;
  }

  const cache = [];
  const cacheWorkloads = [
    ['aos-record-scan', prepareAos, aosScan],
    ['soa-field-scan', prepareSoa, soaScan],
    ['aos-whole-record', prepareWholeAos, wholeAosScan],
    ['soa-whole-record', prepareWholeSoa, wholeSoaScan],
    ['aos-value32-scan', prepareValue32, value32Scan],
    ['soa-f64-scan', preparePackedF64, packedF64Scan],
    ['header-hot-prefix', prepareHeader192, headerHotPrefix],
    ['header-cold-tail', prepareHeader192, headerColdTail],
    ['sequential-f64', prepareSequential, sequentialF64],
    ['random-f64', prepareRandom, randomF64],
    ['pointer-chase', preparePointer, pointerChase],
    ['index-chase', prepareIndex, indexChase],
  ];
  for (const [name, prepare, scan] of cacheWorkloads) {
    if (Date.now() - started >= MAX_TOTAL_MS) break;
    const row = runTimed(name, prepare, scan, CACHE_ITERS, CACHE_REPEATS, started);
    if (row) cache.push(row);
  }

  const machine = machineRecord();
  const shareIters = Math.min(40_000, CACHE_ITERS);
  if (Date.now() - started < MAX_TOTAL_MS) {
    cache.push({
      workload: 'false-share-skipped',
      iterations: shareIters,
      reason: 'disabled: worker startup cannot be safely bounded without a platform-specific process watchdog',
    });
  }

  console.log(JSON.stringify({
    harness: 'bench-ops',
    generated_at: new Date().toISOString(),
    machine,
    budget: {
      enabled: budgetEnabled,
      // Cache probes are diagnostic only: their p95 is machine-specific and
      // therefore has no stable default budget.
      scope: 'workloads',
      max_per_op_ns: budgetEnabled ? MAX_PER_OP_NS : null,
      failures: budgetFailures,
      passed: budgetFailures.length === 0,
    },
    results: output,
    cache_probes: cache,
  }, null, 2));
  if (budgetEnabled && budgetFailures.length) process.exitCode = 1;
}

main();

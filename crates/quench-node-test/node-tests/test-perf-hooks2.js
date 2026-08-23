const { createHistogram, monitorEventLoopDelay, timerify, performance } = require('node:perf_hooks');
const h = createHistogram();
h.record(2); h.record(8); h.add(5);
if (h.min !== 2 || h.max !== 8 || h.mean !== 5 || h.percentile(50) !== 5) throw new Error('histogram stats');
if (h.exceeds(4) !== 2 || h.delta().count !== 3) throw new Error('histogram methods');
h.reset(); if (h.count !== 0) throw new Error('histogram reset');
let called = false;
const wrapped = timerify(function work() { called = true; return 42; });
if (wrapped() !== 42 || !called) throw new Error('timerify result');
if (performance.getEntriesByType('function').length < 1) throw new Error('timerify entry');
const m = monitorEventLoopDelay({ resolution: 2 });
if (m.enable() !== m || m.start() !== m || typeof m.stop !== 'function') throw new Error('monitor controls');
setTimeout(() => {
  m.disable();
  if (m.count < 1 || m.min < 0 || m.max < m.min || typeof m.percentile(99) !== 'number') throw new Error('monitor metrics');
  m.reset(); if (m.count !== 0) throw new Error('monitor reset');
  console.log('perf_hooks2: histogram timerify monitor ok');
}, 12);

// RSS and heap around the user's example. Run with --expose-gc.
const fs = require('fs');
const N = 1_000_000;
function status(field) {
  const line = fs.readFileSync('/proc/self/status', 'utf8').split('\n').find((l) => l.startsWith(field + ':'));
  return Number(line.split(/\s+/)[1]);
}
function sample(label) {
  gc(); gc();
  const heapUsed = process.memoryUsage().heapUsed;
  const row = { label, rssKiB: status('VmRSS'), hwmKiB: status('VmHWM'), heapUsed };
  console.log(JSON.stringify(row));
  return row;
}
function heavyTask() {
  const massiveData = new Array(N).fill('data');
  return function () { return massiveData.length; };
}
const base = sample('baseline');
const holey = new Array(N);
const afterHoley = sample('new Array(N) held');
const started = process.hrtime.bigint();
let runTask = heavyTask();
const elapsedMs = Number(process.hrtime.bigint() - started) / 1e6;
const held = sample('closure held');
console.log(JSON.stringify({
  holeyHeapBytesPerElement: ((afterHoley.heapUsed - base.heapUsed) / N).toFixed(2),
  filledHeapBytesPerElement: ((held.heapUsed - afterHoley.heapUsed) / N).toFixed(2),
  filledRssBytesPerElement: (((held.rssKiB - afterHoley.rssKiB) * 1024) / N).toFixed(2),
  heavyTaskMs: elapsedMs.toFixed(1),
}));
if (runTask() !== N || holey.length !== N) throw new Error('length');
runTask = null;
setTimeout(() => {
  const dropped = sample('closure dropped');
  console.log(JSON.stringify({ residualHeapBytes: dropped.heapUsed - afterHoley.heapUsed, residualRssKiB: dropped.rssKiB - afterHoley.rssKiB }));
}, 0);

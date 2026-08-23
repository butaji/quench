// Node compat: process.uptime() and process.memoryUsage() (real host-backed).
if (typeof process.uptime !== 'function') throw new Error('no process.uptime');
if (typeof process.memoryUsage !== 'function') throw new Error('no process.memoryUsage');

const u1 = process.uptime();
if (!(u1 >= 0)) throw new Error('uptime negative: ' + u1);

const m = process.memoryUsage();
if (typeof m !== 'object' || m === null) throw new Error('memoryUsage not object');
for (const key of ['rss', 'heapTotal', 'heapUsed', 'external', 'arrayBuffers']) {
  if (!(typeof m[key] === 'number' && m[key] >= 0)) {
    throw new Error('memoryUsage.' + key + ' invalid: ' + m[key]);
  }
}

// Wait a short interval and verify uptime monotonically increases.
const start = Date.now();
const uStart = process.uptime();
setTimeout(function () {
  const uAfter = process.uptime();
  const elapsedMs = Date.now() - start;
  if (uAfter < uStart) throw new Error('uptime did not advance: ' + uStart + ' -> ' + uAfter);
  console.log('process-uptime-memusage: ok');
}, 30);
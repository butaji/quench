// Node compat: v8.writeHeapSnapshot (Bun-green; stubs a placeholder file).
const v8 = require('node:v8');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');

if (typeof v8.writeHeapSnapshot !== 'function') throw new Error('no writeHeapSnapshot');

const tmp = path.join(os.tmpdir(), 'quench-heap-' + Date.now() + '-' + process.pid + '.heapsnapshot');
const out = v8.writeHeapSnapshot(tmp);
if (out !== tmp) throw new Error('returned path mismatch: ' + out);
if (!fs.existsSync(out)) throw new Error('file not created');
const size = fs.statSync(out).size;
if (size <= 0) throw new Error('file empty');
fs.unlinkSync(out);

// Default name fallback.
const defaultOut = v8.writeHeapSnapshot();
if (typeof defaultOut !== 'string' || defaultOut.length === 0) {
  throw new Error('default name missing: ' + defaultOut);
}
if (fs.existsSync(defaultOut)) fs.unlinkSync(defaultOut);

console.log('v8-write-heap-snapshot: ok');
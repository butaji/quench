const assert = require('assert');
const { WASI } = require('wasi');

assert.strictEqual(typeof WASI, 'function');
const wasi = new WASI({ args: ['one'], env: { MODE: 'test' } });
assert.strictEqual(typeof wasi.start, 'function');
assert.strictEqual(typeof wasi.initialize, 'function');
assert.strictEqual(typeof wasi.getImportObject, 'function');
assert.strictEqual(wasi.args[0], 'one');
assert.strictEqual(wasi.env.MODE, 'test');
assert.strictEqual(wasi.returnOnExit, true);
assert.strictEqual(wasi.getImportObject().wasi_snapshot_preview1, wasi.wasiImport);
assert.throws(() => wasi.start({ exports: {} }), { name: 'TypeError' });
let started = false;
assert.strictEqual(wasi.start({ exports: { _start() { started = true; return 7; } } }), 7);
assert.strictEqual(started, true);

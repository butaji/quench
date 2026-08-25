const assert = require('assert');
const dc = require('diagnostics_channel');
let seen = 0;
dc.subscribe('quench-stage', () => { seen++; });
dc.channel('quench-stage').publish({});
assert.strictEqual(seen, 1);
const buffer = require('buffer');
assert.strictEqual(buffer.isAscii(new Uint8Array([1, 2])), true);
const encoded = new (require('util').TextEncoder)().encode('hello');
assert.strictEqual(buffer.isAscii(encoded), true);
const { internalBinding } = require('internal/test/binding');
assert.throws(() => internalBinding('buffer').fill(Buffer.alloc(1), 1, -1, 0, 1), {
  code: 'ERR_OUT_OF_RANGE',
});
const detached = new ArrayBuffer(1);
const view = new Uint8Array(detached);
view[0] = 0xff;
structuredClone(detached, { transfer: [detached] });
assert.strictEqual(buffer.isAscii(view), true);

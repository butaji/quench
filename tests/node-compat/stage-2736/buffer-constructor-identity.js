const assert = require('assert');
const { Buffer } = require('buffer');

assert.ok(Buffer.allocUnsafeSlow(4) instanceof Buffer);
assert.ok(Buffer.from(new ArrayBuffer(4)) instanceof Buffer);

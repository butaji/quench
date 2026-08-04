const assert = require("assert");
const vm = require("vm");
const { Buffer } = require("buffer");

const backing = vm.runInNewContext("new ArrayBuffer(4)");
const view = Buffer.from(backing);

assert.strictEqual(view.length, 4);
assert.strictEqual(Buffer.byteLength(backing), 4);
view[0] = 7;
assert.strictEqual(new Uint8Array(backing)[0], 7);

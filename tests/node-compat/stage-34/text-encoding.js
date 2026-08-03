const assert = require("assert");
const encoder = new TextEncoder();
assert.strictEqual(encoder.encode("hello").length, 5);
assert.strictEqual(encoder.encode("\u0222").length, 2);
assert.strictEqual(Buffer.byteLength("\u0222"), 2);

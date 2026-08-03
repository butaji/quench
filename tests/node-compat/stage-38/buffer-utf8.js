const assert = require("assert");
const { Buffer } = require("node:buffer");
const text = "\u0222abc.";
assert.strictEqual(Buffer.from(text).toString(), text);
assert.strictEqual(Buffer.byteLength(text), 6);

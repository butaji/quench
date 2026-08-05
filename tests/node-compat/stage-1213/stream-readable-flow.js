const assert = require("assert");
const { Readable } = require("stream");

const readable = new Readable({ read() {} });
assert.strictEqual(readable.resume(), readable);
assert.strictEqual(readable.pause(), readable);

const assert = require("assert");
const web = require("stream/web");
assert.strictEqual(typeof web.ReadableStream, "function");
assert.strictEqual(web.ReadableStream, ReadableStream);

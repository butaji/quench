const assert = require("assert");
const stream = require("stream");
const promises = require("stream/promises");

assert.strictEqual(stream.promises.pipeline, promises.pipeline);
assert.strictEqual(stream.promises.finished, promises.finished);

const assert = require("assert");
const stream = require("stream");
const { promisify } = require("util");

assert.strictEqual(promisify(stream.pipeline), stream.promises.pipeline);
assert.strictEqual(promisify(stream.finished), stream.promises.finished);

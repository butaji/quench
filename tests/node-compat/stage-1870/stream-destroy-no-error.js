const assert = require("assert");
const { Writable } = require("stream");

const stream = new Writable();
stream.destroy();
assert.strictEqual(stream.errored, null);

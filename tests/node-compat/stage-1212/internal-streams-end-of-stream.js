const assert = require("assert");
const internal = require("internal/streams/end-of-stream");

assert.strictEqual(typeof internal.kEosNodeSynchronousCallback, "symbol");

const assert = require("assert");

const signal = AbortSignal.timeout(1);
assert.strictEqual(signal.aborted, false);
assert.strictEqual(typeof signal.throwIfAborted, "function");

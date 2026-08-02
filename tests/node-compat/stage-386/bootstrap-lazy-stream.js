const assert = require("assert");
assert.strictEqual(globalThis.__nodeStreamInitialized, false);
assert.strictEqual(typeof require("stream").Readable, "function");
assert.strictEqual(globalThis.__nodeStreamInitialized, true);

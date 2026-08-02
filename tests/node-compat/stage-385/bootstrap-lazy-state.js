const assert = require("assert");
assert.strictEqual(globalThis.__nodeCryptoInitialized, false);
require("crypto").randomUUID;
assert.strictEqual(globalThis.__nodeCryptoInitialized, true);

const assert = require("assert");
const crypto = require("crypto");
const bytes = crypto.randomBytes(32);
assert.strictEqual(bytes.length, 32);
const target = Buffer.alloc(8);
assert.strictEqual(crypto.randomFillSync(target, 2, 4), target);

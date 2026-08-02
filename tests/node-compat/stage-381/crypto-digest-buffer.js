const assert = require("assert");
const crypto = require("crypto");
const digest = crypto.createHash("sha256").update("abc").digest();
assert.ok(digest instanceof Uint8Array);
assert.strictEqual(digest.length, 32);

"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const bytes = crypto.randomBytes(32);
assert.ok(Buffer.isBuffer(bytes));
assert.strictEqual(bytes.length, 32);
assert.strictEqual(crypto.randomBytes(0).length, 0);

console.log("crypto random bytes passed");

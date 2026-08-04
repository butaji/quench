"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const digest = crypto.createHash("sha256").update("quench").digest();
assert.ok(Buffer.isBuffer(digest));
assert.strictEqual(digest.length, 32);
assert.strictEqual(digest.toString("hex").slice(0, 16), "a8b51e95fe15708a");

console.log("crypto hash buffer passed");

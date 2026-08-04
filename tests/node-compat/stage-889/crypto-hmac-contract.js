"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const hmac = crypto.createHmac("sha256", "secret");
assert.strictEqual(typeof hmac.update, "function");
assert.strictEqual(typeof hmac.digest, "function");
assert.strictEqual(hmac.update("quench"), hmac);
assert.strictEqual(typeof hmac.digest("hex"), "string");

console.log("crypto hmac contract passed");

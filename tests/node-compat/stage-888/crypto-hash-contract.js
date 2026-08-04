"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const hash = crypto.createHash("sha256");
assert.strictEqual(typeof hash.update, "function");
assert.strictEqual(typeof hash.digest, "function");
assert.strictEqual(hash.update("quench"), hash);
assert.strictEqual(typeof hash.digest("hex"), "string");

console.log("crypto hash contract passed");

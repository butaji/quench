"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const buffer = Buffer.alloc(16);
assert.strictEqual(typeof crypto.randomFillSync, "function");
assert.strictEqual(crypto.randomFillSync(buffer), buffer);
assert.strictEqual(buffer.length, 16);

console.log("crypto random fill sync passed");

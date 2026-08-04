"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

assert.strictEqual(typeof crypto.webcrypto, "object");
assert.strictEqual(typeof crypto.webcrypto.subtle, "object");
assert.strictEqual(typeof crypto.webcrypto.getRandomValues, "function");
assert.strictEqual(typeof crypto.webcrypto.randomUUID, "function");
const values = crypto.webcrypto.getRandomValues(new Uint8Array(4));
assert.strictEqual(values.length, 4);
assert.strictEqual(typeof crypto.webcrypto.randomUUID(), "string");

console.log("crypto webcrypto api passed");

"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const values = new Uint8Array(32);
assert.strictEqual(typeof crypto.webcrypto.getRandomValues, "function");
assert.strictEqual(crypto.webcrypto.getRandomValues(values), values);
assert.ok(values.some((value) => value !== 0));

console.log("crypto webcrypto random passed");

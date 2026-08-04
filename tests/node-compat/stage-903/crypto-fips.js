"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

assert.strictEqual(typeof crypto.getFips, "function");
assert.strictEqual(typeof crypto.setFips, "function");
assert.strictEqual(crypto.getFips(), 0);
assert.strictEqual(crypto.setFips(0), undefined);

console.log("crypto FIPS state passed");

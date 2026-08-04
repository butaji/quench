"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const hashes = crypto.getHashes();
const ciphers = crypto.getCiphers();

assert.ok(Array.isArray(hashes));
assert.ok(hashes.includes("sha256"));
assert.ok(Array.isArray(ciphers));
assert.ok(ciphers.length > 0);

console.log("crypto algorithm inventory passed");

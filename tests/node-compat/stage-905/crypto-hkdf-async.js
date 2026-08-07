"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

assert.strictEqual(
  crypto.hkdf("sha256", "ikm", "salt", "info", 16, (error, output) => {
    assert.ifError(error);
    assert.ok(output instanceof ArrayBuffer);
    assert.strictEqual(output.byteLength, 16);
  }),
  undefined,
);

console.log("crypto HKDF async passed");

"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

assert.strictEqual(
  crypto.pbkdf2("password", "salt", 2, 32, "sha256", (error, output) => {
    assert.ifError(error);
    assert.ok(Buffer.isBuffer(output));
    assert.strictEqual(output.length, 32);
  }),
  undefined,
);

console.log("crypto PBKDF2 async passed");

"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const output = crypto.pbkdf2Sync("password", "salt", 2, 32, "sha256");
assert.ok(Buffer.isBuffer(output));
assert.strictEqual(
  output.toString("hex"),
  "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43",
);

console.log("crypto PBKDF2 passed");

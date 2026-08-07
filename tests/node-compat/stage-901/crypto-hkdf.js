"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const output = crypto.hkdfSync(
  "sha256",
  Buffer.from("ikm"),
  Buffer.from("salt"),
  Buffer.from("info"),
  16,
);
assert.ok(output instanceof ArrayBuffer);
assert.strictEqual(
  Buffer.from(output).toString("hex"),
  "fe8f9615d2374c0d17f77d1aeaf408c2",
);

console.log("crypto HKDF passed");

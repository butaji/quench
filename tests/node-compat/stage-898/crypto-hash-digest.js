"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const digest = crypto.createHash("sha256").update("quench").digest("hex");
assert.strictEqual(
  digest,
  "a8b51e95fe15708a5f253f567e72f00f052cd6c11f013b19c5b122bc52b98073",
);

console.log("crypto hash digest passed");

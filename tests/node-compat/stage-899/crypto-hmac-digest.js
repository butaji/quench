"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

const digest = crypto
  .createHmac("sha256", "secret")
  .update("quench")
  .digest("hex");
assert.strictEqual(
  digest,
  "b0442d3dde155df3b9c85aa381513c903b03bdbe9c7d57dd0da0ad6df5c81d8a",
);

console.log("crypto HMAC digest passed");

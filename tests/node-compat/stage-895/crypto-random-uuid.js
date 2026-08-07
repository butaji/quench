"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

assert.strictEqual(typeof crypto.randomUUID, "function");
const uuid = crypto.randomUUID();
assert.match(
  uuid,
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
);

console.log("crypto random uuid passed");

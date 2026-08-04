"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

assert.strictEqual(typeof crypto.randomInt, "function");
for (let attempt = 0; attempt < 20; attempt += 1) {
  const value = crypto.randomInt(10, 20);
  assert.ok(Number.isInteger(value));
  assert.ok(value >= 10 && value < 20);
}

console.log("crypto random int passed");

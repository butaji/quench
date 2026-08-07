"use strict";

const assert = require("assert");
const crypto = require("node:crypto");

assert.strictEqual(typeof crypto.timingSafeEqual, "function");
assert.strictEqual(
  crypto.timingSafeEqual(Buffer.from("quench"), Buffer.from("quench")),
  true,
);
assert.throws(
  () => crypto.timingSafeEqual(Buffer.from("quench"), Buffer.from("quench!")),
  {
    name: "RangeError",
  },
);

console.log("crypto timing safe equal passed");

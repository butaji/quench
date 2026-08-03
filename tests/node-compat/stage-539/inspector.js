"use strict";

const assert = require("assert");

for (const name of [
  "inspector",
  "node:inspector",
  "inspector/promises",
  "node:inspector/promises"
]) {
  assert.throws(() => require(name), { code: "ERR_UNKNOWN_BUILTIN_MODULE" });
}

console.log("inspector error passed");

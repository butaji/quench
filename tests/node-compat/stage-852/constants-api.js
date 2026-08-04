"use strict";

const assert = require("assert");
const constants = require("node:constants");

for (const name of ["errno", "signals", "os", "fs", "crypto", "zlib"]) {
  assert.strictEqual(typeof constants[name], "object");
}
assert.strictEqual(typeof constants.O_RDONLY, "number");
assert.strictEqual(typeof constants.SIGTERM, "number");

console.log("constants api passed");

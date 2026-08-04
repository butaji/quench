"use strict";

const assert = require("assert");
const bufferApi = require("node:buffer");

for (const name of ["atob", "btoa"]) {
  assert.strictEqual(typeof bufferApi[name], "function");
}
assert.strictEqual(typeof bufferApi.constants, "object");
assert.strictEqual(typeof bufferApi.kMaxLength, "number");
assert.strictEqual(bufferApi.btoa("ok"), "b2s=");
assert.strictEqual(bufferApi.atob("b2s="), "ok");

console.log("buffer modern api passed");

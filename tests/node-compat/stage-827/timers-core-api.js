"use strict";

const assert = require("assert");
const timersApi = require("node:timers");
const promisesApi = require("node:timers/promises");

for (
  const name of [
    "setTimeout",
    "clearTimeout",
    "setImmediate",
    "clearImmediate",
    "setInterval",
    "clearInterval",
  ]
) {
  assert.strictEqual(typeof timersApi[name], "function");
}
for (const name of ["setTimeout", "setImmediate", "setInterval"]) {
  assert.strictEqual(typeof promisesApi[name], "function");
}
assert.strictEqual(typeof promisesApi.scheduler, "object");

console.log("timers core api passed");

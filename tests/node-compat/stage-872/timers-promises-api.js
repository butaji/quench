"use strict";

const assert = require("assert");
const timers = require("node:timers/promises");

for (const name of ["setTimeout", "setImmediate", "setInterval"]) {
  assert.strictEqual(typeof timers[name], "function");
}
assert.strictEqual(typeof timers.scheduler, "object");
assert.strictEqual(typeof timers.scheduler.wait, "function");
assert.strictEqual(typeof timers.scheduler.yield, "function");

console.log("timers promises api passed");

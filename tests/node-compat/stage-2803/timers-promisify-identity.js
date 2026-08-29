"use strict";
const assert = require("assert");
const timers = require("timers");
const timersPromises = require("timers/promises");
const { promisify } = require("util");

assert.strictEqual(promisify(timers.setTimeout), timersPromises.setTimeout);
promisify(timers.setTimeout)(0, "ok").then((value) => {
  assert.strictEqual(value, "ok");
});

let called = false;
const timeout = setTimeout(() => {
  called = true;
}, 1);
clearTimeout(String(timeout));
setTimeout(() => assert.strictEqual(called, false), 5);

"use strict";
const assert = require("assert");
const timers = require("timers");
const timersPromises = require("timers/promises");
const { promisify } = require("util");

assert.strictEqual(promisify(timers.setTimeout), timersPromises.setTimeout);
promisify(timers.setTimeout)(0, "ok").then((value) => {
  assert.strictEqual(value, "ok");
});

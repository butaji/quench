"use strict";
const assert = require("assert");
const timers = require("timers");
const promises = require("timers/promises");
assert.strictEqual(require("util").promisify(timers.setTimeout), promises.setTimeout);

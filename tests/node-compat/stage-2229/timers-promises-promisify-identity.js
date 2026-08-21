const assert = require("assert");
const timers = require("timers");
const timerPromises = require("timers/promises");
const { promisify } = require("util");

assert.strictEqual(promisify(timers.setTimeout), timerPromises.setTimeout);
assert.strictEqual(promisify(timers.setImmediate), timerPromises.setImmediate);
console.log("timers promises promisify identity passed");

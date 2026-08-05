const assert = require("assert");
const timers = require("timers");
const promises = require("timers/promises");

assert.deepStrictEqual(promises, timers.promises);

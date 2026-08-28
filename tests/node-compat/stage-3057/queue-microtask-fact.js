const assert = require("assert");
const timers = require("timers");

assert.strictEqual(typeof queueMicrotask, "function");
assert.strictEqual(typeof timers.setTimeout, "function");
queueMicrotask(() => assert.ok(true));

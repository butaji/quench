"use strict";

const assert = require("assert");
const { mock } = require("node:test");

const original = AbortSignal.timeout;
mock.timers.enable({ apis: ["AbortSignal.timeout"] });

const signal = AbortSignal.timeout(10);
assert.strictEqual(signal.aborted, false);
mock.timers.tick(9);
assert.strictEqual(signal.aborted, false);
mock.timers.tick(1);
assert.strictEqual(signal.aborted, true);

mock.timers.reset();
assert.strictEqual(AbortSignal.timeout, original);

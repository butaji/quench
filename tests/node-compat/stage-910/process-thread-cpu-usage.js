"use strict";

const assert = require("assert");

const current = process.threadCpuUsage();
assert.ok(Number.isFinite(current.user));
assert.ok(Number.isFinite(current.system));
assert.throws(() => process.threadCpuUsage(123), TypeError);
assert.throws(() => process.threadCpuUsage([]), TypeError);

console.log("process thread CPU usage passed");

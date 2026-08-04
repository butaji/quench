"use strict";

const assert = require("assert");

assert.ok(Number.isInteger(process.pid));
assert.ok(process.pid > 0);
assert.ok(Number.isInteger(process.ppid));
assert.ok(process.ppid >= 0);

console.log("process parent PID passed");

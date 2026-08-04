"use strict";

const assert = require("assert");
const processApi = require("process");

const previous = processApi.cpuUsage();
const current = processApi.cpuUsage(previous);
assert.strictEqual(typeof current.user, "number");
assert.strictEqual(typeof current.system, "number");

console.log("process cpuUsage previous sample passed");

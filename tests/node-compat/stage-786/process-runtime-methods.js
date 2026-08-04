"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.openStdin, "function");
assert.strictEqual(processApi.openStdin(), processApi.stdin);
assert.strictEqual(typeof processApi.constrainedMemory, "function");
assert.strictEqual(typeof processApi.constrainedMemory(), "number");
assert.strictEqual(typeof processApi.threadCpuUsage, "function");
const usage = processApi.threadCpuUsage();
assert.strictEqual(typeof usage.user, "number");
assert.strictEqual(typeof usage.system, "number");

console.log("process runtime methods passed");

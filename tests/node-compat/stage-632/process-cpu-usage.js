"use strict";

const assert = require("assert");
const processApi = require("process");

const usage = processApi.cpuUsage();
assert(usage && typeof usage === "object");
assert.strictEqual(typeof usage.user, "number");
assert.strictEqual(typeof usage.system, "number");

console.log("process cpuUsage passed");

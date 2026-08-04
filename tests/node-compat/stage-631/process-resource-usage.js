"use strict";

const assert = require("assert");
const processApi = require("process");

const usage = processApi.resourceUsage();
assert(usage && typeof usage === "object");
assert.strictEqual(typeof usage.userCPUTime, "number");
assert.strictEqual(typeof usage.systemCPUTime, "number");
assert.strictEqual(typeof usage.maxRSS, "number");

console.log("process resourceUsage passed");

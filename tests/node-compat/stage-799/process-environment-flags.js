"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.allowedNodeEnvironmentFlags instanceof Set, true);
assert.strictEqual(processApi.allowedNodeEnvironmentFlags.size > 0, true);

console.log("process environment flags passed");

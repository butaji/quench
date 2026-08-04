"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.platform, "string");
assert(processApi.platform.length > 0);
assert.strictEqual(typeof processApi.arch, "string");
assert(processApi.arch.length > 0);

console.log("process platform and arch passed");

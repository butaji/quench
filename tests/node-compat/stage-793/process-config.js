"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.config, "object");
assert.strictEqual(typeof processApi.config.variables, "object");
assert.strictEqual(typeof processApi.config.target_defaults, "object");

console.log("process config passed");

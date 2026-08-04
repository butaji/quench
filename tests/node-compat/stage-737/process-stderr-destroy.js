"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.destroy, "function");
assert.strictEqual(processApi.stderr.destroyed, false);
assert.strictEqual(processApi.stderr.destroy(), processApi.stderr);
assert.strictEqual(processApi.stderr.destroyed, false);

console.log("process stderr destroy passed");

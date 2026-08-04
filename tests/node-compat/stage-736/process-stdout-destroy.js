"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.destroy, "function");
assert.strictEqual(processApi.stdout.destroyed, false);
assert.strictEqual(processApi.stdout.destroy(), processApi.stdout);
assert.strictEqual(processApi.stdout.destroyed, false);

console.log("process stdout destroy passed");

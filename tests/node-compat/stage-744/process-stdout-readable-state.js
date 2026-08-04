"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.readable, false);
assert.strictEqual(processApi.stdout.readableEnded, true);
assert.strictEqual(processApi.stdout.readableFlowing, null);

console.log("process stdout readable state passed");

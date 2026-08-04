"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.readable, false);
assert.strictEqual(processApi.stderr.readableEnded, true);
assert.strictEqual(processApi.stderr.readableFlowing, null);

console.log("process stderr readable state passed");

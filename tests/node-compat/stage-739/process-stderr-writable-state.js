"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stderr.writable, true);
assert.strictEqual(processApi.stderr.writableEnded, false);
assert.strictEqual(processApi.stderr.writableFinished, false);

console.log("process stderr writable state passed");

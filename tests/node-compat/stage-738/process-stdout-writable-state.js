"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdout.writable, true);
assert.strictEqual(processApi.stdout.writableEnded, false);
assert.strictEqual(processApi.stdout.writableFinished, false);

console.log("process stdout writable state passed");

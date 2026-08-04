"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.emit, "function");
assert.strictEqual(processApi.stdout.emit("drain"), false);

console.log("process stdout emit passed");

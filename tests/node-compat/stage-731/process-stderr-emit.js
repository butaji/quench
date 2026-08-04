"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.emit, "function");
assert.strictEqual(processApi.stderr.emit("drain"), false);

console.log("process stderr emit passed");

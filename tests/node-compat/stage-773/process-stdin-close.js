"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdin.close, "function");
assert.strictEqual(processApi.stdin.pending, false);
assert.strictEqual(processApi.stdin.close(), processApi.stdin);

console.log("process stdin close passed");

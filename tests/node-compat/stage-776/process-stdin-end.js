"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.end, null);
assert.strictEqual(processApi.stdin.pos, undefined);
assert.strictEqual(processApi.stdin.start, undefined);

console.log("process stdin end passed");

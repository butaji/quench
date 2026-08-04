"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.closed, false);
assert.strictEqual(processApi.stdin.errored, null);
assert.strictEqual(processApi.stdin.readableAborted, false);
assert.strictEqual(processApi.stdin.autoClose, false);
assert.strictEqual(processApi.stdin.bytesRead, 0);

console.log("process stdin stream state passed");

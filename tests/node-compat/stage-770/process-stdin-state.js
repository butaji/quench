"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.fd, 0);
assert.strictEqual(processApi.stdin.destroyed, false);
assert.strictEqual(processApi.stdin.readableEncoding, null);

console.log("process stdin state passed");

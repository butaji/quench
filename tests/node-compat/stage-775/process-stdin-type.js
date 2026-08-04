"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(processApi.stdin.constructor.name, "ReadStream");
assert.strictEqual(processApi.stdin.isTTY, undefined);
assert.strictEqual(processApi.stdin.writable, undefined);

console.log("process stdin type passed");

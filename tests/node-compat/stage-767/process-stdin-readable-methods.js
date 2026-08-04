"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdin.read, "function");
assert.strictEqual(processApi.stdin.read(), null);
assert.strictEqual(typeof processApi.stdin.unshift, "function");
assert.strictEqual(processApi.stdin.unshift(Buffer.alloc(0)), processApi.stdin);

console.log("process stdin readable methods passed");

"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdin.pause, "function");
assert.strictEqual(typeof processApi.stdin.resume, "function");
assert.strictEqual(processApi.stdin.pause(), processApi.stdin);
assert.strictEqual(processApi.stdin.resume(), processApi.stdin);

console.log("process stdin flow passed");

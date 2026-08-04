"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdin.pipe, "function");
assert.strictEqual(typeof processApi.stdin.unpipe, "function");
assert.strictEqual(typeof processApi.stdin.wrap, "function");
const destination = {};
assert.strictEqual(processApi.stdin.pipe(destination), destination);
assert.strictEqual(processApi.stdin.unpipe(), processApi.stdin);
assert.strictEqual(processApi.stdin.wrap(), processApi.stdin);

console.log("process stdin readable methods passed");

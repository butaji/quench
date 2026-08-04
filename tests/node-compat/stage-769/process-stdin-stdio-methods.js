"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdin.destroy, "function");
assert.strictEqual(typeof processApi.stdin.ref, "function");
assert.strictEqual(typeof processApi.stdin.unref, "function");
assert.strictEqual(processApi.stdin.ref(), processApi.stdin);
assert.strictEqual(processApi.stdin.unref(), processApi.stdin);

console.log("process stdin stdio methods passed");

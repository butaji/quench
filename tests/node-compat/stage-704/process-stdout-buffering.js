"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.cork, "function");
assert.strictEqual(typeof processApi.stdout.uncork, "function");
assert.strictEqual(processApi.stdout.cork(), processApi.stdout);
assert.strictEqual(processApi.stdout.uncork(), processApi.stdout);

console.log("process stdout buffering passed");

"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.cork, "function");
assert.strictEqual(typeof processApi.stderr.uncork, "function");
assert.strictEqual(processApi.stderr.cork(), processApi.stderr);
assert.strictEqual(processApi.stderr.uncork(), processApi.stderr);

console.log("process stderr buffering passed");

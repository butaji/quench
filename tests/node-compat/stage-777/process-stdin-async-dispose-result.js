"use strict";

const assert = require("assert");
const processApi = require("process");

const result = processApi.stdin[Symbol.asyncDispose]();
assert.strictEqual(result instanceof Promise, true);

console.log("process stdin async dispose result passed");

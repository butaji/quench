"use strict";

const assert = require("assert");
const processApi = require("process");

assert(processApi.stdin && typeof processApi.stdin === "object");
assert.strictEqual(typeof processApi.stdin.on, "function");

console.log("process stdin passed");

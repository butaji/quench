"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.title, "string");
assert.strictEqual(processApi.title, "node");

console.log("process title passed");

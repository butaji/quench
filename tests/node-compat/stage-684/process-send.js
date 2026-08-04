"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.send, "function");
assert.strictEqual(typeof processApi.send({ stage: 684 }), "boolean");

console.log("process send passed");

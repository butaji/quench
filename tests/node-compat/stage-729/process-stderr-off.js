"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.off, "function");
const listener = () => {};
assert.strictEqual(processApi.stderr.off("drain", listener), processApi.stderr);

console.log("process stderr off passed");

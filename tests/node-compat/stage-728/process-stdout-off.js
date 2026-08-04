"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.off, "function");
const listener = () => {};
assert.strictEqual(processApi.stdout.off("drain", listener), processApi.stdout);

console.log("process stdout off passed");

"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.addListener, "function");
const listener = () => {};
assert.strictEqual(
  processApi.stdout.addListener("drain", listener),
  processApi.stdout,
);
processApi.stdout.removeListener("drain", listener);

console.log("process stdout addListener passed");

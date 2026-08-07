"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.addListener, "function");
const listener = () => {};
assert.strictEqual(
  processApi.stderr.addListener("drain", listener),
  processApi.stderr,
);
processApi.stderr.removeListener("drain", listener);

console.log("process stderr addListener passed");

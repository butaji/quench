"use strict";

const assert = require("assert");
const processApi = require("process");

const listener = () => {};
assert.strictEqual(
  processApi.stderr.once("drain", listener),
  processApi.stderr,
);
processApi.stderr.removeListener("drain", listener);

console.log("process stderr once passed");

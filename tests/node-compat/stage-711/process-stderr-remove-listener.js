"use strict";

const assert = require("assert");
const processApi = require("process");

const listener = () => {};
processApi.stderr.on("drain", listener);
assert.strictEqual(
  processApi.stderr.removeListener("drain", listener),
  processApi.stderr,
);

console.log("process stderr removeListener passed");

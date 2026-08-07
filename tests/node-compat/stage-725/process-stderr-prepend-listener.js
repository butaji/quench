"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.prependListener, "function");
const listener = () => {};
assert.strictEqual(
  processApi.stderr.prependListener("drain", listener),
  processApi.stderr,
);
processApi.stderr.removeListener("drain", listener);

console.log("process stderr prependListener passed");

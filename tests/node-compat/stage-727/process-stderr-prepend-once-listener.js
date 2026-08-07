"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stderr.prependOnceListener, "function");
const listener = () => {};
assert.strictEqual(
  processApi.stderr.prependOnceListener("drain", listener),
  processApi.stderr,
);
processApi.stderr.removeListener("drain", listener);

console.log("process stderr prependOnceListener passed");

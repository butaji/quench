"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.prependOnceListener, "function");
const listener = () => {};
assert.strictEqual(
  processApi.stdout.prependOnceListener("drain", listener),
  processApi.stdout,
);
processApi.stdout.removeListener("drain", listener);

console.log("process stdout prependOnceListener passed");

"use strict";

const assert = require("assert");
const processApi = require("process");

assert.strictEqual(typeof processApi.stdout.prependListener, "function");
const listener = () => {};
assert.strictEqual(
  processApi.stdout.prependListener("drain", listener),
  processApi.stdout,
);
processApi.stdout.removeListener("drain", listener);

console.log("process stdout prependListener passed");

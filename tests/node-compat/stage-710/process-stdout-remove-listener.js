"use strict";

const assert = require("assert");
const processApi = require("process");

const listener = () => {};
processApi.stdout.on("drain", listener);
assert.strictEqual(
  processApi.stdout.removeListener("drain", listener),
  processApi.stdout,
);

console.log("process stdout removeListener passed");

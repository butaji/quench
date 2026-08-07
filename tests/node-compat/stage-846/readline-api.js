"use strict";

const assert = require("assert");
const readline = require("node:readline");
const promises = require("node:readline/promises");

for (
  const name of [
    "createInterface",
    "emitKeypressEvents",
    "cursorTo",
    "moveCursor",
    "clearLine",
  ]
) {
  assert.strictEqual(typeof readline[name], "function");
}
for (const name of ["Interface", "ReadStream", "WriteStream"]) {
  assert.strictEqual(typeof readline[name], "function");
}
assert.strictEqual(typeof promises.Interface, "function");

console.log("readline api passed");

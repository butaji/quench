"use strict";

const assert = require("assert");
const fs = require("node:fs");

for (
  const name of [
    "opendir",
    "opendirSync",
    "Dir",
    "Dirent",
    "ReadStream",
    "WriteStream",
  ]
) {
  assert.strictEqual(typeof fs[name], "function");
}
assert.strictEqual(typeof fs.promises.opendir, "function");

console.log("fs directory api passed");

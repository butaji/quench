"use strict";

const assert = require("assert");
const pathApi = require("node:path");

for (
  const name of [
    "resolve",
    "normalize",
    "isAbsolute",
    "join",
    "relative",
    "toNamespacedPath",
    "dirname",
    "basename",
    "extname",
    "format",
    "parse",
  ]
) {
  assert.strictEqual(typeof pathApi[name], "function");
}
assert.strictEqual(typeof pathApi.sep, "string");
assert.strictEqual(typeof pathApi.delimiter, "string");
assert.strictEqual(typeof pathApi.win32, "object");
assert.strictEqual(typeof pathApi.posix, "object");
assert.strictEqual(pathApi.join("a", "b"), "a/b");

console.log("path core api passed");

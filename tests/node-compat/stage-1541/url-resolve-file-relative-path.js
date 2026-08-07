const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(url.resolve("../abc", "file:/e/x/y/z"), "file:///e/x/abc");
assert.strictEqual(
  url.resolve("/example/x/abc", "file:/example2/x/y/z"),
  "file:///example/x/abc",
);
console.log("file URL relative paths passed");

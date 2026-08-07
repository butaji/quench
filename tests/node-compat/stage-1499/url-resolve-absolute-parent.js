const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.resolve("/foo/bar/baz", "/../etc/passwd"),
  "/etc/passwd",
);
console.log("url resolve absolute parent passed");

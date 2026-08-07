const assert = require("node:assert");
const url = require("node:url");

assert.strictEqual(
  url.format({ protocol: "file", pathname: "/home/user", path: "/home/user" }),
  "file:///home/user",
);
assert.strictEqual(
  url.format({ protocol: "file:", pathname: "/tmp/a b" }),
  "file:///tmp/a%20b",
);
console.log("file object format matrix passed");

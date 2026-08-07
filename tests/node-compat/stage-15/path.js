const assert = require("assert");
const path = require("path");
assert.strictEqual(path.normalize("/tmp/../var/./log"), "/var/log");
assert.strictEqual(path.isAbsolute("/var/log"), true);
assert.strictEqual(path.relative("/a/b", "/a/c/d"), "../c/d");
assert.strictEqual(path.parse("/tmp/file.txt").name, "file");
assert.strictEqual(
  path.format({ dir: "/tmp", base: "file.txt" }),
  "/tmp/file.txt",
);

const assert = require("node:assert");
const path = require("node:path");

assert.strictEqual(path.resolve("/tmp/", "file.txt"), "/tmp/file.txt");
assert.strictEqual(path.resolve("/tmp/", "directory/"), "/tmp/directory/");

console.log("path resolve trailing passed");

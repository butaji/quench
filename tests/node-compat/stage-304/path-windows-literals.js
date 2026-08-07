const assert = require("assert");
const path = require("path");

assert.strictEqual(
  path.win32.normalize("C:\\\\foo\\bar\\..\\baz"),
  "C:\\foo\\baz",
);
assert.strictEqual(path.win32.basename("C:\\\\tmp\\file.txt", ".txt"), "file");

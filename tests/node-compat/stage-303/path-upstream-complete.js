const assert = require("assert");
const path = require("path");

assert.strictEqual(path.posix.normalize("/foo//bar/../baz"), "/foo/baz");
assert.strictEqual(path.posix.join("foo", "bar", "..", "baz"), "foo/baz");
assert.strictEqual(
  path.win32.normalize("C:\\\\foo\\bar\\..\\baz"),
  "C:\\foo\\baz",
);
assert.strictEqual(path.win32.basename("C:\\\\tmp\\file.txt", ".txt"), "file");

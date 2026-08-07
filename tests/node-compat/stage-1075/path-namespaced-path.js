const assert = require("assert");
const path = require("path");

assert.strictEqual(path.posix.toNamespacedPath("/foo/bar"), "/foo/bar");
assert.strictEqual(path.win32.toNamespacedPath("C:/foo"), "\\\\?\\C:\\foo");
assert.strictEqual(
  path.win32.toNamespacedPath("\\\\foo\\bar"),
  "\\\\?\\UNC\\foo\\bar\\",
);
assert.strictEqual(path.win32.toNamespacedPath(null), null);

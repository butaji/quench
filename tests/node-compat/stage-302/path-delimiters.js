const assert = require("assert");
const path = require("path");

assert.strictEqual(path.posix.sep, "/");
assert.strictEqual(path.posix.delimiter, ":");
assert.strictEqual(path.win32.sep, "\\");
assert.strictEqual(path.win32.delimiter, ";");

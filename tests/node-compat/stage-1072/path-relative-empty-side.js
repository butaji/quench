const assert = require("assert");
const path = require("path");

assert.strictEqual(path.win32.relative("C:\\", "C:\\a\\b"), "a\\b");
assert.strictEqual(path.win32.relative("C:\\a\\b", "C:\\"), "..\\..");

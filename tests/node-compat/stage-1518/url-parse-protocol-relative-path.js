const assert = require("node:assert");
const url = require("node:url");

const parsed = url.parse("//some_path");
assert.strictEqual(parsed.pathname, "//some_path");
assert.strictEqual(parsed.path, "//some_path");
assert.strictEqual(parsed.href, "//some_path");
console.log("url parse protocol-relative path passed");

const assert = require("assert");
const path = require("path");

assert.strictEqual(path.basename(__filename), "single-file-path-context.js");
assert.ok(path.isAbsolute(__filename));

const assert = require("node:assert");
const path = require("path");
const common = require("../common");
assert.strictEqual(path.basename("/tmp/example.txt"), "example.txt");
assert.strictEqual(path.dirname("/tmp/example.txt"), "/tmp");
assert.strictEqual(path.extname("example.txt"), ".txt");
const callback = common.mustCall(() => {});
assert.strictEqual(typeof callback, "function");
callback();

const assert = require("node:assert");
const fs = require("node:fs");

const resolved = fs.realpathSync.native(
  "./test/parallel/test-fs-realpath-native.js",
);
assert.strictEqual(
  resolved.endsWith("tests/node/test/parallel/test-fs-realpath-native.js"),
  true,
);

console.log("realpath fixture relative path passed");

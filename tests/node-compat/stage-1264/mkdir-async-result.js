const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const root = path.join(
  process.cwd(),
  `tests/node/test/.tmp.0/mkdir-async-${Date.now()}`,
);
fs.mkdir(path.join(root, "child"), { recursive: true }, (error, created) => {
  assert.ifError(error);
  assert.strictEqual(created, root);
});

console.log("async mkdir result passed");

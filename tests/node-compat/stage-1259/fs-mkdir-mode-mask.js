const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const directory = path.join(
  process.cwd(),
  `tests/node/test/.tmp.0/mkdir-mode-${Date.now()}`,
);
fs.mkdirSync(directory, 0o10644);
assert.strictEqual(fs.statSync(directory).mode & 0o777, 0o644);

console.log("mkdir mode mask passed");

const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const prefix = path.join(
  process.cwd(),
  "tests/node/test/.tmp.0/quench-mkdtemp-",
);
const directory = fs.mkdtempSync(prefix);
assert.strictEqual(
  path.basename(directory).length,
  "quench-mkdtemp-XXXXXX".length,
);
assert.strictEqual(fs.existsSync(directory), true);

console.log("mkdtemp absolute prefix passed");

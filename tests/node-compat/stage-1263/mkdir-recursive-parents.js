const assert = require("node:assert");
const fs = require("node:fs");
const path = require("node:path");

const root = path.join(
  process.cwd(),
  `tests/node/test/.tmp.0/mkdir-parent-${Date.now()}`,
);
const nested = path.join(root, "child");
const first = fs.mkdirSync(nested, { recursive: true });

assert.strictEqual(fs.existsSync(nested), true);
assert.strictEqual(first, root);

console.log("mkdir recursive parents passed");

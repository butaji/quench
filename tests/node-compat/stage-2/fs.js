const assert = require("node:assert");
const fs = require("fs");
const path = require("path");
const folder = fs.mkdtempSync(path.join("/tmp", "quench-node-"));
assert.strictEqual(fs.existsSync(folder), true);
assert.strictEqual(path.basename(folder).startsWith("quench-node-"), true);

const assert = require("assert");
const fs = require("fs");
const folder = fs.mkdtempSync("/tmp/quench-node-");
assert.strictEqual(
  fs.realpathSync(folder).endsWith(folder.split("/").pop()),
  true,
);
fs.rmSync(folder, { recursive: true });
assert.strictEqual(fs.existsSync(folder), false);

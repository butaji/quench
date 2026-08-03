const assert = require("assert");
const fs = require("fs");
const path = require("path");
const folder = fs.mkdtempSync("/tmp/quench-node-");
assert.strictEqual(path.basename(folder).length, "quench-node-XXXXXX".length);
fs.rmdirSync(folder);

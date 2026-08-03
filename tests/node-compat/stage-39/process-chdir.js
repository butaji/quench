const assert = require("assert");
const fs = require("fs");
const folder = fs.mkdtempSync("/tmp/quench-node-");
const before = process.cwd();
process.chdir(folder);
assert.strictEqual(process.cwd(), fs.realpathSync(folder));
process.chdir(before);
fs.rmdirSync(folder);

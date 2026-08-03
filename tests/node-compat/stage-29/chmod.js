const assert = require("assert");
const fs = require("fs");
const folder = fs.mkdtempSync("/tmp/quench-node-");
const file = `${folder}/file`;
fs.writeFileSync(file, "x");
fs.chmodSync(file, 0o600);
assert.strictEqual(fs.statSync(file).isFile(), true);
fs.unlinkSync(file);
fs.rmdirSync(folder);

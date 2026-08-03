const assert = require("assert");
const fs = require("fs");
const folder = fs.mkdtempSync("/tmp/quench-node-");
const nested = `${folder}/nested`;
fs.mkdirSync(nested);
fs.writeFileSync(`${nested}/file.txt`, "x");
assert.deepStrictEqual(fs.readdirSync(nested), ["file.txt"]);
fs.rmdirSync(folder);
assert.strictEqual(fs.existsSync(folder), false);

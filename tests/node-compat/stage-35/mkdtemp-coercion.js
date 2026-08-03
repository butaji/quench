const assert = require("assert");
const fs = require("fs");
const folder = fs.mkdtempSync("/tmp/quench-node-");
const encoded = new TextEncoder().encode(`${folder}/bytes.`);
const created = fs.mkdtempSync(encoded);
assert.strictEqual(fs.existsSync(created), true);
fs.rmdirSync(created);
fs.rmdirSync(folder);

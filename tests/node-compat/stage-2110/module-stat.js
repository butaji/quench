const assert = require("assert");
const fs = require("fs");
const moduleApi = require("module");

const directory = "/tmp/quench-module-stat-directory";
const file = "/tmp/quench-module-stat-file.js";
fs.rmSync(directory, { recursive: true, force: true });
fs.rmSync(file, { force: true });
fs.mkdirSync(directory);
fs.writeFileSync(file, "module.exports = 1;");

assert.strictEqual(moduleApi._stat(directory), 1);
assert.strictEqual(moduleApi._stat(file), 0);
assert.ok(moduleApi._stat("/tmp/quench-module-stat-missing") < 0);

console.log("module stat pass");

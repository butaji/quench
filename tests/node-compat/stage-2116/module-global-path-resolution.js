const assert = require("assert");
const fs = require("fs");
const path = require("path");
const moduleApi = require("module");

const root = "/tmp/quench-module-global-path";
const packageRoot = path.join(root, "global-package");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(packageRoot, { recursive: true });
fs.writeFileSync(
  path.join(packageRoot, "index.js"),
  "module.exports = 'global';"
);
process.env.NODE_PATH = root;
moduleApi._initPaths();

assert.strictEqual(require("global-package"), "global");
console.log("module global path resolution pass");

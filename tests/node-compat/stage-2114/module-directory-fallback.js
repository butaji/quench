const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = "/tmp/quench-module-directory-fallback";
const packageRoot = path.join(root, "node_modules", "bare-package");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(packageRoot, { recursive: true });
fs.writeFileSync(
  path.join(packageRoot, "index.js"),
  "module.exports = { legacy: true };"
);
fs.writeFileSync(
  path.join(root, "entry.js"),
  "module.exports = require('bare-package');"
);

assert.deepStrictEqual(require(path.join(root, "entry.js")), { legacy: true });
console.log("module directory fallback pass");

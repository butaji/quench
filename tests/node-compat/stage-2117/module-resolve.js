const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = "/tmp/quench-module-resolve";
const packageRoot = path.join(root, "node_modules", "resolve-package");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(packageRoot, { recursive: true });
fs.writeFileSync(path.join(packageRoot, "index.js"), "module.exports = true;");
fs.writeFileSync(path.join(root, "entry.js"), "module.exports = true;");

const entry = require(path.join(root, "entry.js"));
assert.strictEqual(
  require.resolve(path.join(root, "entry.js")),
  fs.realpathSync(path.join(root, "entry.js"))
);
assert.strictEqual(typeof require.resolve.paths("resolve-package"), "object");
assert.ok(Array.isArray(require.resolve.paths("resolve-package")));
assert.strictEqual(require.resolve("path"), "path");
assert.strictEqual(require.resolve.paths("path"), null);
assert.strictEqual(entry, true);

console.log("module resolve pass");

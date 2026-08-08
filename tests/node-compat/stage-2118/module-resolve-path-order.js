const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = "/tmp/quench-module-resolve-path-order";
const outer = path.join(root, "node_modules");
const nested = path.join(outer, "node_modules");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(nested, { recursive: true });
fs.writeFileSync(path.join(outer, "ordered.js"), "module.exports = 'outer';");
fs.writeFileSync(path.join(nested, "ordered.js"), "module.exports = 'nested';");
fs.writeFileSync(path.join(root, "entry.js"), "module.exports = true;");

const paths = [nested, outer];
assert.strictEqual(
  require.resolve("ordered", { paths }),
  fs.realpathSync(path.join(outer, "ordered.js"))
);
console.log("module resolve path order pass");

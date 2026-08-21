const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = "/tmp/quench-module-trailing-package";
const packageRoot = path.join(root, "node_modules", "main-package");
fs.rmSync(root, { recursive: true, force: true });
fs.mkdirSync(path.join(packageRoot, "lib"), { recursive: true });
fs.writeFileSync(
  path.join(packageRoot, "package.json"),
  JSON.stringify({ main: "./lib/index.js" }),
);
fs.writeFileSync(
  path.join(packageRoot, "lib", "index.js"),
  "module.exports = 'main';",
);
fs.writeFileSync(
  path.join(root, "entry.js"),
  "module.exports = require('main-package/');",
);

assert.strictEqual(require(path.join(root, "entry.js")), "main");
assert.strictEqual(
  require.resolve("main-package/", {
    paths: [path.join(root, "node_modules")],
  }),
  fs.realpathSync(path.join(packageRoot, "lib", "index.js")),
);
console.log("module trailing slash package pass");

const assert = require("assert");
const moduleApi = require("module");

assert.deepStrictEqual(
  moduleApi._nodeModulePaths("/usr/test/lib/node_modules/npm/foo"),
  [
    "/usr/test/lib/node_modules/npm/foo/node_modules",
    "/usr/test/lib/node_modules/npm/node_modules",
    "/usr/test/lib/node_modules",
    "/usr/test/node_modules",
    "/usr/node_modules",
    "/node_modules"
  ]
);
assert.deepStrictEqual(moduleApi._nodeModulePaths("/node_modules"), [
  "/node_modules"
]);
assert.deepStrictEqual(moduleApi._nodeModulePaths("/"), ["/node_modules"]);

console.log("module node module paths pass");

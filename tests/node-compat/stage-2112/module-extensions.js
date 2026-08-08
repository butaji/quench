const assert = require("assert");
const fs = require("fs");
const moduleApi = require("module");

const filename = "/tmp/quench-module-extension.foo";
fs.writeFileSync(filename, "ignored");
require.extensions[".foo"] = (loaded, loadedFilename) => {
  assert.strictEqual(loadedFilename, filename);
  loaded.exports = { extension: ".foo" };
};

assert.deepStrictEqual(require(filename), { extension: ".foo" });
assert.strictEqual(require.extensions, moduleApi._extensions);
delete require.extensions[".foo"];

console.log("module extensions pass");

const assert = require("assert");
const vm = require("vm");

const sourceModule = new vm.SourceTextModule(
  'import value from "dep"; import "side-effect"; export default value;',
);

assert.deepStrictEqual(sourceModule.dependencySpecifiers, ["dep", "side-effect"]);
assert.strictEqual(sourceModule.dependencySpecifiers, sourceModule.dependencySpecifiers);

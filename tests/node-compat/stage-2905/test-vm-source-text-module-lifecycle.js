const assert = require("assert");
const vm = require("vm");
const util = require("util");

(async () => {
  const module = new vm.SourceTextModule("export const a = 1; export var b = 2");
  await module.link(() => 0);
  assert.strictEqual(
    util.inspect(module.namespace),
    "[Module: null prototype] { a: <uninitialized>, b: undefined }",
  );
  await module.evaluate();
  assert.strictEqual(
    util.inspect(module.namespace),
    "[Module: null prototype] { a: 1, b: 2 }",
  );
  console.log("source text module lifecycle passed");
})();

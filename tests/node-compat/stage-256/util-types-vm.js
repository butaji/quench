const { types } = require("util");
const vm = require("vm");

const module = new vm.SourceTextModule("");
module.link(() => 0);
module.evaluate();
if (!types.isModuleNamespaceObject(module.namespace)) {
  throw new Error("module namespace check failed");
}
if (types.isKeyObject() || types.isCryptoKey()) {
  throw new Error("empty key checks failed");
}

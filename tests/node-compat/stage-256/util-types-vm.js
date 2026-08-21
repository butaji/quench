const { types } = require("util");
const vm = require("vm");

const sourceModule = new vm.SourceTextModule("");
sourceModule.link(() => 0);
sourceModule.evaluate();
if (!types.isModuleNamespaceObject(sourceModule.namespace)) {
  throw new Error("module namespace check failed");
}
if (types.isKeyObject() || types.isCryptoKey()) {
  throw new Error("empty key checks failed");
}

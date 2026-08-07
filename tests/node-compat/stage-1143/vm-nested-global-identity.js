const vm = require("vm");

const sandbox = { document: null };
sandbox.document = { defaultView: sandbox };
vm.createContext(sandbox);
vm.runInContext(
  "Object.defineProperty(this, 'foo', { get() { return document.defaultView; } }); result = foo === this;",
  sandbox,
);
if (sandbox.result !== true) throw new Error("nested global identity was lost");

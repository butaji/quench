const vm = require("vm");
const context = vm.createContext();
const script = new vm.Script('"passed";');
if (script.runInContext(context) !== "passed") {
  throw new Error("Script evaluation failed");
}

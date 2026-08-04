const vm = require("vm");
const context = vm.createContext();
context.window = context;
if (vm.runInContext("this", context) !== vm.runInContext("window", context)) {
  throw new Error("context global and window are not identical");
}

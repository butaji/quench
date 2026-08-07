const vm = require("vm");
const context = vm.createContext({});
vm.runInContext('Object.defineProperty(this, "x", { value: 42 })', context);
if (vm.runInContext("x", context) !== 42) {
  throw new Error("non-configurable context value changed");
}
vm.runInContext("x = 0", context);
if (vm.runInContext("x", context) !== 42) {
  throw new Error("read-only context value changed");
}

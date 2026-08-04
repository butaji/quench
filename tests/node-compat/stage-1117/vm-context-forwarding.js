const vm = require("vm");
const sandbox = { x: 3 };
const context = vm.createContext(sandbox);
if (vm.runInContext("x", context) !== 3) throw new Error("context read failed");
vm.runInContext("y = 4", context);
if (sandbox.y !== 4 || context.y !== 4) throw new Error("context write failed");

const vm = require("vm");
const context = vm.createContext({});
const value = vm.runInContext("1 + 1", context);
if (value !== 2) throw new Error(String(value));

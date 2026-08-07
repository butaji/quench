const vm = require("vm");

const context = vm.createContext({});
if (
  vm.runInContext("typeof process + ':' + typeof Object", context) !==
    "undefined:function"
) {
  throw new Error("contextified sandbox exposed an unexpected host global");
}

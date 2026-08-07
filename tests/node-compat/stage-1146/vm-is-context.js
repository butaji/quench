const vm = require("vm");

for (const value of ["string", null, undefined, 8.9, Symbol("sym"), true]) {
  try {
    vm.isContext(value);
    throw new Error("primitive context input was accepted");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}
if (vm.isContext({}) || vm.isContext([])) {
  throw new Error("plain object was a context");
}
if (!vm.isContext(vm.createContext([]))) {
  throw new Error("context was not tracked");
}

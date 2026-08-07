const vm = require("vm");
try {
  vm.createContext("string is not supported");
  throw new Error("primitive context was accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
if (vm.createContext({ a: 1 }).a !== 1) {
  throw new Error("object context changed");
}
if (vm.createContext([0, 1]).length !== 2) {
  throw new Error("array context changed");
}

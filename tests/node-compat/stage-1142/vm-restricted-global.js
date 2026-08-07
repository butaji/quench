const vm = require("vm");

const context = vm.createContext({});
vm.runInContext(
  "Object.defineProperty(this, 'foo', { value: 1, configurable: false });",
  context,
);
try {
  vm.runInContext("let foo = 2;", context);
  throw new Error("restricted global declaration was accepted");
} catch (error) {
  if (error.name !== "SyntaxError") throw error;
}

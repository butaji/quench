const vm = require("vm");

const context = vm.createContext({ value: "ok" });
const compiled = vm.compileFunction("return value;", [], {
  parsingContext: context,
});
if (compiled() !== "ok") throw new Error("parsing context was not used");

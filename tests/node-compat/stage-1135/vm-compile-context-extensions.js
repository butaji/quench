const vm = require("vm");

const compiled = vm.compileFunction("return value;", [], {
  contextExtensions: [{ value: 7 }],
});
if (compiled() !== 7) throw new Error("context extension was not visible");

try {
  vm.compileFunction("", [], { contextExtensions: null });
  throw new Error("invalid context extensions were accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}

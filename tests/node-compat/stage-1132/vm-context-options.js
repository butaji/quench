const vm = require("vm");

for (const value of [null, "string"]) {
  try {
    vm.createContext({}, value);
    throw new Error("invalid context options were accepted");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}

try {
  vm.createContext({}, { name: null });
  throw new Error("invalid context name was accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}

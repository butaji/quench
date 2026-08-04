const vm = require("vm");

for (const value of [null, {}, [1], "bad", true]) {
  try {
    new vm.Script("void 0", { lineOffset: value });
    throw new Error("lineOffset accepted invalid input");
  } catch (error) {
    if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
  }
}

for (const value of [0.1, 2 ** 32]) {
  try {
    new vm.Script("void 0", { columnOffset: value });
    throw new Error("columnOffset accepted out-of-range input");
  } catch (error) {
    if (error.code !== "ERR_OUT_OF_RANGE") throw error;
  }
}

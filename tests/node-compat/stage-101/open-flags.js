const { stringToFlags } = require("internal/fs/utils");

if (
  stringToFlags("r") !== 0 ||
  stringToFlags("w") !== 577 ||
  stringToFlags("a") !== 1089
) {
  throw new Error("flag mapping mismatch");
}
try {
  stringToFlags("invalid");
  throw new Error("invalid flag accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_VALUE") throw error;
}

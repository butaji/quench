const buffer = require("buffer");

for (const value of [NaN, -1]) {
  try {
    buffer.INSPECT_MAX_BYTES = value;
    throw new Error("invalid inspect limit accepted");
  } catch (error) {
    if (error.code !== "ERR_OUT_OF_RANGE") throw error;
  }
}
try {
  buffer.INSPECT_MAX_BYTES = "many";
  throw new Error("string inspect limit accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}

const { Buffer } = require("buffer");
try {
  Buffer.concat(["bad"]);
  throw new Error("invalid concat item was accepted");
} catch (error) {
  if (
    error.code !== "ERR_INVALID_ARG_TYPE" ||
    !error.message.includes("list[0]")
  ) {
    throw new Error("concat item validation was not descriptive");
  }
}

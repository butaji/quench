const buffer = require("buffer");

if (buffer.kMaxLength !== buffer.constants.MAX_LENGTH) {
  throw new Error("buffer max length mismatch");
}
if (typeof buffer.constants.MAX_STRING_LENGTH !== "number") {
  throw new Error("buffer string max length missing");
}

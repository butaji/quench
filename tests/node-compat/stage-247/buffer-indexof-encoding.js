const { Buffer } = require("buffer");

if (Buffer.from("abcdef").indexOf("64", "HEX") !== 3) {
  throw new Error("indexOf encoding overload failed");
}

const { Buffer } = require("buffer");

if (Buffer.from("=bad", "base64").length !== 0) {
  throw new Error("invalid base64 prefix was decoded");
}

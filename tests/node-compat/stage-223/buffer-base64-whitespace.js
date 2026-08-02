const { Buffer } = require("buffer");

const encoded = "aGVs\n bG8=";
if (Buffer.from(encoded, "base64").toString() !== "hello") {
  throw new Error("base64 whitespace handling failed");
}

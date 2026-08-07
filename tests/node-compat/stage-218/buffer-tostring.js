const { Buffer } = require("buffer");

const buffer = Buffer.from("hello");
if (buffer.toString("BASE64URL") !== "aGVsbG8") {
  throw new Error("base64url failed");
}
if (Buffer.from("666f6f", "HEX").toString("HEX") !== "666f6f") {
  throw new Error("case-insensitive hex failed");
}

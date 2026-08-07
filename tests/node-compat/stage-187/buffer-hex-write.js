const { Buffer } = require("buffer");

const buffer = Buffer.alloc(4);
if (buffer.write("abcdxx", 0, "hex") !== 2) {
  throw new Error("hex write count mismatch");
}
if (buffer.toString("hex") !== "abcd0000") {
  throw new Error("hex write data mismatch");
}
if (Buffer.from("xxabcd", "hex").length !== 0) {
  throw new Error("invalid hex prefix accepted");
}
if (Buffer.from("abcdxx", "hex").toString("hex") !== "abcd") {
  throw new Error("invalid hex suffix accepted");
}

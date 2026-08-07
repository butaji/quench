const { Buffer } = require("buffer");

const buffer = Buffer.alloc(8).fill("ab", 2, 6, "utf8");
if (buffer.toString("hex") !== "0000616261620000") {
  throw new Error("string fill failed");
}
buffer.fill(0xff, 0, 2);
if (buffer[0] !== 255 || buffer[1] !== 255) {
  throw new Error("numeric fill failed");
}

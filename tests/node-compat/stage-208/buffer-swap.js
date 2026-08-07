const { Buffer } = require("buffer");

const buffer = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]);
if (
  buffer.swap16() !== buffer ||
  buffer.toString("hex") !== "0201040306050807"
) {
  throw new Error("swap16 failed");
}
buffer.swap16();
buffer.swap32();
if (buffer.toString("hex") !== "0403020108070605") {
  throw new Error("swap32 failed");
}
buffer.swap32();
buffer.swap64();
if (buffer.toString("hex") !== "0807060504030201") {
  throw new Error("swap64 failed");
}

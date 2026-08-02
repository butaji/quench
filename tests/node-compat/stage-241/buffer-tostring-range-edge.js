const { Buffer } = require("buffer");

const buffer = Buffer.from("abc");
if (buffer.toString("ascii", 1, -100) !== "") {
  throw new Error("negative end range failed");
}

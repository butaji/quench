const { Buffer } = require("buffer");

const buffer = Buffer.alloc(5, "800A", "hex");
if (buffer.toString("hex") !== "800a800a80") {
  throw new Error("encoded alloc fill failed");
}

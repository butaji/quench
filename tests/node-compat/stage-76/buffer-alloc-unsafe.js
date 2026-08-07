const { Buffer } = require("buffer");

const value = Buffer.allocUnsafe(8);
if (!Buffer.isBuffer(value) || value.length !== 8) {
  throw new Error("allocUnsafe contract mismatch");
}

const { Buffer } = require("buffer");

if (Buffer.alloc(3.3).length !== 3 || Buffer.allocUnsafe(3.3).length !== 3) {
  throw new Error("fractional buffer size was not truncated");
}

const buffer = require("buffer");
const { Buffer } = buffer;

buffer.INSPECT_MAX_BYTES = 1;
if (Buffer.from([1, 2]).inspect() !== "<Buffer 01 ... 1 more byte>") {
  throw new Error("live inspect limit failed");
}

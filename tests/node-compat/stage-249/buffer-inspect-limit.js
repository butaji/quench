const buffer = require("buffer");
const { Buffer } = buffer;

if (Buffer.INSPECT_MAX_BYTES !== undefined) {
  throw new Error("Buffer.INSPECT_MAX_BYTES should be undefined");
}
buffer.INSPECT_MAX_BYTES = 1;
if (Buffer.from([1, 2]).inspect() !== "<Buffer 01 ... 1 more byte>") {
  throw new Error("live inspect limit failed");
}

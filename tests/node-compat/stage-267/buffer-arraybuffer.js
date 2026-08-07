const { Buffer } = require("buffer");

const arrayBuffer = new ArrayBuffer(4);
if (Buffer.from(arrayBuffer, 0, "invalid").length !== 0) {
  throw new Error("non-numeric length mismatch");
}
try {
  Buffer.from(arrayBuffer, 0, Infinity);
  throw new Error("infinite length accepted");
} catch (error) {
  if (error.code !== "ERR_BUFFER_OUT_OF_BOUNDS") throw error;
}

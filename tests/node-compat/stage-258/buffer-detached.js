const { Buffer } = require("buffer");

const arrayBuffer = new ArrayBuffer(8);
structuredClone(arrayBuffer, { transfer: [arrayBuffer] });
try {
  Buffer.isAscii(arrayBuffer);
  throw new Error("detached buffer was accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_STATE") throw error;
}

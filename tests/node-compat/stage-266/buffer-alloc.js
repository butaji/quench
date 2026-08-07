const { Buffer } = require("buffer");

const values = new Uint32Array(4).fill(42);
const copied = Buffer.from(values);
if (copied.length !== 4 || copied.some((value) => value !== 42)) {
  throw new Error("typed element conversion failed");
}
if (Buffer.from({ buffer: new ArrayBuffer(0) }).length !== 0) {
  throw new Error("buffer object conversion failed");
}
try {
  Buffer.alloc(1, Buffer.alloc(0));
  throw new Error("empty view pattern accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_VALUE") throw error;
}

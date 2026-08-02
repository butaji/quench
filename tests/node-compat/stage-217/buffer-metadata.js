const { Buffer } = require("buffer");

const array = new ArrayBuffer(2);
const buffer = Buffer.from(array);
if (buffer.parent !== array || buffer.buffer !== array) {
  throw new Error("Buffer parent metadata failed");
}
if (buffer.offset !== 0) throw new Error("Buffer offset metadata failed");
if (typeof Buffer.poolSize !== "number") throw new Error("poolSize missing");

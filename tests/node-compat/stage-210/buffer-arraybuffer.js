const { Buffer } = require("buffer");

const array = new Uint8Array([1, 2, 3, 4]);
const buffer = Buffer.from(array.buffer, 1, 2);
if (buffer.toString("hex") !== "0203") {
  throw new Error("ArrayBuffer view failed");
}
buffer[0] = 9;
if (array[1] !== 9) throw new Error("ArrayBuffer sharing failed");

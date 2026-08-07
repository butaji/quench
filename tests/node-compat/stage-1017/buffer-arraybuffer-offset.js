const { Buffer } = require("buffer");
const source = new Uint8Array([1, 2, 3, 4, 5]);
const buffer = Buffer.from(source.buffer, 1, 3);
if (buffer.length !== 3 || buffer[0] !== 2 || buffer[2] !== 4) {
  throw new Error("ArrayBuffer offset range was not preserved");
}
buffer[0] = 9;
if (source[1] !== 9) throw new Error("offset Buffer did not share storage");

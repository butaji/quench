const { Buffer } = require("buffer");
const arrayBuffer = new ArrayBuffer(4);
const view = new Uint8Array(arrayBuffer);
const buffer = Buffer.from(arrayBuffer);
if (!(buffer instanceof Buffer) || buffer.buffer !== arrayBuffer) {
  throw new Error("Buffer.from did not retain the ArrayBuffer");
}
buffer.fill(12);
if (view[0] !== 12 || buffer.length !== 4) {
  throw new Error("Buffer.from did not share ArrayBuffer storage");
}

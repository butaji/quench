const { Buffer } = require("buffer");

const shared = new SharedArrayBuffer(4);
const view = new Uint16Array(shared);
view[0] = 5000;
const buffer = Buffer.from(shared);
if (buffer.length !== 4 || Buffer.byteLength(shared) !== 4) {
  throw new Error("SharedArrayBuffer conversion failed");
}
view[0] = 6000;
if (buffer[0] !== 0x70 || buffer[1] !== 0x17) {
  throw new Error("shared backing storage failed");
}

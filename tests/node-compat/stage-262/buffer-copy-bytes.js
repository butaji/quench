const { Buffer } = require("buffer");

const values = new Uint16Array([0, 0xffff]);
const copied = Buffer.copyBytesFrom(values, 1, 5);
if (copied.length !== 2 || copied[0] !== 255 || copied[1] !== 255) {
  throw new Error("typed-array copy failed");
}
if (Buffer.copyBytesFrom(new Uint8Array([1, 2]), 10).length !== 0) {
  throw new Error("past-end copy failed");
}

const { Buffer } = require("buffer");

const values = new Uint16Array([0, 0xffff]);
const copy = Buffer.copyBytesFrom(values, 1, 3);
if (copy.length !== 2 || copy.toString("hex") !== "ffff") {
  throw new Error("copyBytesFrom failed");
}
if (
  Buffer.copyBytesFrom(new Uint8Array([1, 2, 3])).toString("hex") !== "010203"
) {
  throw new Error("default offset/length failed");
}

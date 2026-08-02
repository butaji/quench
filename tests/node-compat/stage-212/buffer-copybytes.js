const { Buffer } = require("buffer");

const values = new Uint16Array([0, 0xffff]);
const copy = Buffer.copyBytesFrom(values, 1, 3);
if (copy.length !== 3 || copy.toString("hex") !== "00ffff") {
  throw new Error("copyBytesFrom failed");
}

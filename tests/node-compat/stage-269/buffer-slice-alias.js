const { Buffer } = require("buffer");

const buffer = Buffer.from([1, 2, 3, 4]);
const slice = buffer.slice(1, 3);
slice[0] = 9;
if (buffer[1] !== 9) throw new Error("slice did not share backing storage");
slice.swap16();
if (buffer[1] !== 3 || buffer[2] !== 9) {
  throw new Error("slice mutation did not reach parent");
}

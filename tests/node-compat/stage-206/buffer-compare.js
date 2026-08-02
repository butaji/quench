const { Buffer } = require("buffer");

const left = Buffer.from("aa");
const right = new Uint8Array([97, 97]);
if (left.compare(right) !== 0 || Buffer.compare(left, right) !== 0) {
  throw new Error("Buffer compare failed");
}
if (Buffer.compare(Buffer.from("a"), Buffer.from("b")) >= 0) {
  throw new Error("Buffer ordering failed");
}

const { Buffer } = require("buffer");
const left = Buffer.from("aa");
const right = new Uint8Array([97, 97]);
if (
  left.compare(right) !== 0 || Buffer.compare(left, Buffer.from("c")) !== -1
) {
  throw new Error("buffer compare");
}

const { Buffer } = require("buffer");

const buffer = Buffer.alloc(16);
buffer.writeDoubleBE(-2, 0);
buffer.writeDoubleLE(-2, 8);
if (buffer.readDoubleBE(0) !== -2 || buffer.readDoubleLE(8) !== -2) {
  throw new Error("double roundtrip mismatch");
}
if (buffer[0] !== 0xc0 || buffer[8] !== 0) {
  throw new Error("double byte order mismatch");
}
buffer.writeDoubleBE(Infinity, 0);
if (buffer.readDoubleBE(0) !== Infinity) {
  throw new Error("double infinity mismatch");
}

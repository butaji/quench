const { Buffer } = require("buffer");

const buffer = Buffer.alloc(8);
buffer.writeUInt16BE(0x2343, 0);
buffer.writeUInt16LE(0x2343, 2);
buffer.writeUInt32BE(0x12345678, 4);
if (
  buffer.readUInt16BE(0) !== 0x2343 ||
  buffer.readUInt16LE(2) !== 0x2343 ||
  buffer.readUInt32BE(4) !== 0x12345678
) {
  throw new Error("uint roundtrip mismatch");
}
try {
  buffer.readUInt32BE(6);
  throw new Error("accepted out-of-bounds read");
} catch (error) {
  if (error.code !== "ERR_BUFFER_OUT_OF_BOUNDS") throw error;
}

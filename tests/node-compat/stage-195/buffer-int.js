const { Buffer } = require("buffer");

const buffer = Buffer.alloc(6);
buffer.writeInt16BE(-5, 0);
buffer.writeInt16LE(-1679, 2);
if (buffer.readInt16BE(0) !== -5 || buffer.readInt16LE(2) !== -1679) {
  throw new Error("signed fixed mismatch");
}
buffer.writeIntBE(-2, 0, 6);
if (buffer.readIntBE(0, 6) !== -2) throw new Error("signed variable mismatch");

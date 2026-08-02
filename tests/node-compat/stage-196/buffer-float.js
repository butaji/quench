const { Buffer } = require("buffer");

const buffer = Buffer.alloc(8);
buffer.writeFloatBE(1.5, 0);
buffer.writeFloatLE(-2.25, 4);

if (buffer.readFloatBE(0) !== 1.5 || buffer.readFloatLE(4) !== -2.25) {
  throw new Error("Buffer float round-trip failed");
}

if (buffer.writeFloatLE(0, 0) !== 4 || buffer.writeFloatBE(0, 4) !== 8) {
  throw new Error("Buffer float write offset failed");
}

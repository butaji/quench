const { Buffer } = require("buffer");

const buffer = Buffer.alloc(8);
buffer.writeBigInt64LE(-123456789n);
if (buffer.readBigInt64LE() !== -123456789n) {
  throw new Error("signed BigInt failed");
}
buffer.writeBigUInt64BE(123456789n);
if (buffer.readBigUInt64BE() !== 123456789n) {
  throw new Error("unsigned BigInt failed");
}

const { Buffer } = require("buffer");

if (Buffer.prototype.readBigUInt64LE !== Buffer.prototype.readBigUint64LE) {
  throw new Error("BigInt read alias mismatch");
}
if (Buffer.prototype.writeBigUInt64BE !== Buffer.prototype.writeBigUint64BE) {
  throw new Error("BigInt write alias mismatch");
}

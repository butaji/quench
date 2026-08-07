const { Buffer } = require("buffer");

if (Buffer.prototype.readUint32BE !== Buffer.prototype.readUInt32BE) {
  throw new Error("read Uint alias mismatch");
}
if (Buffer.prototype.writeUintLE !== Buffer.prototype.writeUIntLE) {
  throw new Error("write Uint alias mismatch");
}

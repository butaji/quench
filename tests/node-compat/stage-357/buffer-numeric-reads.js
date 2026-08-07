const { Buffer } = require("buffer");
const value = Buffer.from([
  0xa4,
  0xfd,
  0x48,
  0xea,
  0xcf,
  0xff,
  0xd9,
  0x01,
  0xde,
]);
if (value.readInt8(1) !== -3) throw new Error("readInt8");
if (value.readUInt16LE(1) !== 0x48fd) throw new Error("readUInt16LE");
if (value.readInt32BE(1) !== -45552945) throw new Error("readInt32BE");

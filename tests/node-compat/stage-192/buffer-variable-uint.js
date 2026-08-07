const { Buffer } = require("buffer");

const buffer = Buffer.alloc(6);
buffer.writeUIntBE(0x1234567890ab, 0, 6);
if (
  buffer.toString("hex") !== "1234567890ab" ||
  buffer.readUIntBE(0, 6) !== 0x1234567890ab
) {
  throw new Error("variable BE mismatch");
}
buffer.writeUIntLE(0x1234567890ab, 0, 6);
if (
  buffer.toString("hex") !== "ab9078563412" ||
  buffer.readUIntLE(0, 6) !== 0x1234567890ab
) {
  throw new Error("variable LE mismatch");
}

const { Buffer } = require("buffer");

if (Buffer.byteLength("hello", "utf8") !== 5) {
  throw new Error("utf8 length failed");
}
if (Buffer.byteLength("hello", "utf16le") !== 10) {
  throw new Error("utf16 length failed");
}
if (Buffer.byteLength(new Uint16Array(3)) !== 6) {
  throw new Error("typed length failed");
}
if (Buffer.byteLength(new ArrayBuffer(4)) !== 4) {
  throw new Error("array buffer length failed");
}

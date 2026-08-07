const { isAscii, isUtf8, Buffer } = require("buffer");

if (!isAscii(Buffer.from("hello"))) throw new Error("isAscii failed");
if (isAscii(Buffer.from([0xff]))) throw new Error("isAscii accepted non-ASCII");
if (!isUtf8(Buffer.from("hello"))) throw new Error("isUtf8 failed");
if (isUtf8(Buffer.from([0xc0, 0x80]))) {
  throw new Error("isUtf8 accepted overlong input");
}

const { Buffer } = require("buffer");

const buffer = Buffer.alloc(6);
if (buffer.write("abc", 1, "utf8") !== 3) throw new Error("write count failed");
if (buffer.toString("hex") !== "006162630000") {
  throw new Error("write bytes failed");
}
const utf16 = Buffer.alloc(3);
if (utf16.write("x", 1, "utf16le") !== 2) throw new Error("utf16 write failed");

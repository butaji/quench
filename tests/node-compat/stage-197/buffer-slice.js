const { Buffer } = require("buffer");
const buffer = Buffer.from("0123456789");
const sliced = buffer.slice(2, -2);
if (sliced.toString() !== "234567") throw new Error("wrong slice");
if (Buffer("hello").slice(1).toString() !== "ello") {
  throw new Error("callable Buffer failed");
}
if (Buffer.compare(sliced, Buffer.from("234567")) !== 0) {
  throw new Error("Buffer.compare failed");
}

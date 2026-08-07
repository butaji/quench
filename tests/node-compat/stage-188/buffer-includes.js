const { Buffer } = require("buffer");

const buffer = Buffer.from("abcdef");
if (!buffer.includes("bc") || buffer.includes("bc", 2)) {
  throw new Error("buffer string includes mismatch");
}
if (!buffer.includes(0x61) || buffer.includes(0x61, 1)) {
  throw new Error("buffer byte includes mismatch");
}
if (!buffer.includes("", Infinity)) {
  throw new Error("buffer empty includes mismatch");
}
if (!Buffer.from("ΚΑ", "ucs2").includes("Α", 2, "ucs2")) {
  throw new Error("buffer encoding includes mismatch");
}

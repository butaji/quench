const { Buffer } = require("buffer");

if (Buffer.from("ff").indexOf(Buffer.from("f"), 1, "ucs2") !== -1) {
  throw new Error("odd UCS-2 search offset accepted");
}

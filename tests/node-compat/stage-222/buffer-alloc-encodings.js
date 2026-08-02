const { Buffer } = require("buffer");

if (Buffer.from("über", "ascii").toString("hex") !== "fc626572") {
  throw new Error("ascii conversion failed");
}
if (Buffer.from("über", "utf-16le").toString("utf-16le") !== "über") {
  throw new Error("utf16 conversion failed");
}

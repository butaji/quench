const { Buffer } = require("buffer");

if (Buffer.of(1, 2, 257).toString("hex") !== "010201") {
  throw new Error("Buffer.of failed");
}
if (Buffer.allocUnsafeSlow(2).length !== 2) {
  throw new Error("allocUnsafeSlow failed");
}

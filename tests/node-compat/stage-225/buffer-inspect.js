const { Buffer } = require("buffer");

if (Buffer.from([0, 15, 255]).inspect() !== "<Buffer 00 0f ff>") {
  throw new Error("Buffer.inspect failed");
}

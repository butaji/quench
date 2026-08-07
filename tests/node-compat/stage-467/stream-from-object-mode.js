const { Readable } = require("stream");

const stream = Readable.from([1, 2]);
if (stream.readableObjectMode !== true) {
  throw new Error("Readable.from did not enable object mode");
}

console.log("stream from object mode passed");

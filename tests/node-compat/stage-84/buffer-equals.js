const { Buffer } = require("buffer");

if (!Buffer.from([1, 2]).equals(Buffer.from([1, 2]))) {
  throw new Error("Buffer.equals failed");
}
if (Buffer.from([1, 2]).equals(Buffer.from([1, 3]))) {
  throw new Error("Buffer.equals false positive");
}

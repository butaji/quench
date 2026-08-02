const { Buffer } = require("buffer");
const util = require("util");

const buffer = Buffer.from([1, 2]);
buffer.extra = 3;
if (!util.inspect(buffer).includes("extra: 3")) {
  throw new Error("Buffer inspect properties failed");
}

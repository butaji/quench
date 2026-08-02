const { Buffer } = require("buffer");

const result = Buffer.concat([new Uint8Array([1, 2]), Buffer.from([3, 4])], 3);
if (result.toString("hex") !== "010203") throw new Error("concat failed");

if (Buffer.concat([], 4).toString("hex") !== "00000000") {
  throw new Error("concat zero fill failed");
}

const { Buffer } = require("buffer");

const input = Buffer.from([1, 2, 3]);
const empty = Buffer.concat([input], 0);
if (empty.length !== 0) throw new Error("concat length mismatch");
if (!(empty instanceof Buffer)) throw new Error("concat type mismatch");
if (Buffer.concat([input], 2)[0] !== 1 || Buffer.concat([input], 2)[1] !== 2) {
  throw new Error("concat truncation mismatch");
}

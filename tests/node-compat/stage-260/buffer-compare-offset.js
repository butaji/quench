const { Buffer } = require("buffer");

const left = Buffer.from([1, 2, 3, 4]);
const right = Buffer.from([3, 4, 1, 2]);
if (left.compare(right, 2, 4, 0, 2) !== 0) {
  throw new Error("offset compare failed");
}
try {
  left.compare(right, "0");
  throw new Error("string offset accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}

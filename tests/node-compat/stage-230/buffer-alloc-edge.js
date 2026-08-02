const { Buffer } = require("buffer");

if (
  Buffer.prototype.parent !== undefined ||
  Buffer.prototype.offset !== undefined
) {
  throw new Error("prototype metadata should be undefined");
}
if (Buffer.from({ length: -100 }).length !== 0) {
  throw new Error("negative array-like length was not clamped");
}

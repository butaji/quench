const { Buffer } = require("buffer");

let rejected = false;
try {
  Buffer.from({ nope: true });
} catch (error) {
  rejected = error.code === "ERR_INVALID_ARG_TYPE";
}
if (!rejected) throw new Error("invalid Buffer.from input accepted");
if (Buffer.from({ 0: 7, length: 1 })[0] !== 7) {
  throw new Error("array-like Buffer.from failed");
}

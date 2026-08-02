const { Buffer } = require("buffer");

let rejected = false;
try {
  Buffer.alloc(2).fill(1, 0, {});
} catch (error) {
  rejected = error.code === "ERR_INVALID_ARG_TYPE";
}
if (!rejected) throw new Error("invalid fill range accepted");

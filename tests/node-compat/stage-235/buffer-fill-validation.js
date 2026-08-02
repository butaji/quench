const { Buffer } = require("buffer");

let rejected = false;
try {
  Buffer.alloc(4).fill("yKJh", "hex");
} catch (error) {
  rejected = error.code === "ERR_INVALID_ARG_VALUE";
}
if (!rejected) throw new Error("invalid hex fill accepted");

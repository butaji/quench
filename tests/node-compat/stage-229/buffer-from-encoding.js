const { Buffer } = require("buffer");

let rejected = false;
try {
  Buffer.from("value", "buffer");
} catch (error) {
  rejected = error.code === "ERR_UNKNOWN_ENCODING";
}
if (!rejected) throw new Error("unknown Buffer.from encoding accepted");

const { Buffer } = require("buffer");

const buffer = Buffer.alloc(3);
let rejected = false;
try {
  buffer.write("abc", 0, 3, "not-an-encoding");
} catch (error) {
  rejected = error.code === "ERR_UNKNOWN_ENCODING";
}
if (!rejected) throw new Error("unknown write encoding accepted");

const { Buffer } = require("buffer");

let rejected = false;
try {
  Buffer.alloc(4).write("x", "utf8", 0);
} catch (error) {
  rejected = error.code === "ERR_INVALID_ARG_TYPE";
}
if (!rejected) throw new Error("ambiguous write overload accepted");

const { Buffer } = require("buffer");

try {
  new Buffer(4);
  throw new Error("numeric legacy constructor accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
const value = new Buffer("ok");
if (value.toString() !== "ok")
  throw new Error("string legacy constructor failed");

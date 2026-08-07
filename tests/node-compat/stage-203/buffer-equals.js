const { Buffer } = require("buffer");

const value = Buffer.from("abc");
if (!value.equals(new Uint8Array([97, 98, 99]))) {
  throw new Error("equals failed");
}
if (value.equals(Buffer.from("abd"))) throw new Error("equals mismatch failed");
let threw = false;
try {
  value.equals("abc");
} catch (error) {
  threw = error.code === "ERR_INVALID_ARG_TYPE";
}
if (!threw) throw new Error("equals validation failed");

const { Buffer } = require("buffer");
const value = Buffer.from("ab", "hex");
console.log(value.toString("hex"), value.length);
if (value.length !== 1 || value[0] !== 171) {
  throw new Error("hex input did not produce the expected byte");
}
if (value.toString("hex") !== "ab") {
  throw new Error("hex output did not render byte values");
}

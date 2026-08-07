const { Buffer } = require("buffer");

const buffer = Buffer.from("abc");
if (buffer.toString("ascii", -1, 3) !== "abc") {
  throw new Error("negative start was not clamped");
}
if (buffer.toString("ascii", 0, "invalid") !== "") {
  throw new Error("invalid end was not treated as zero");
}
if (buffer.toString("ascii", 0, undefined) !== "abc") {
  throw new Error("undefined end was not defaulted");
}

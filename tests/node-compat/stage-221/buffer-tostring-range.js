const { Buffer } = require("buffer");

const buffer = Buffer.from("hello world");
if (buffer.toString("ascii", 0, 5) !== "hello") {
  throw new Error("toString range failed");
}
if (buffer.toString("ascii", -5) !== "hello world") {
  throw new Error("negative start was not clamped");
}

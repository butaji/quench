const { Buffer } = require("buffer");

const buffer = Buffer.from("abc");
if (buffer.toLocaleString() !== "abc") throw new Error("locale string failed");
if (
  buffer[Symbol.for("nodejs.util.inspect.custom")]() !== "<Buffer 61 62 63>"
) {
  throw new Error("inspect symbol failed");
}

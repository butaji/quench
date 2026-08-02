const { Buffer } = require("buffer");

const buffer = Buffer.from("w00t");
Object.defineProperty(buffer, "length", { value: 1337 });
try {
  buffer.fill("");
  throw new Error("forged length accepted");
} catch (error) {
  if (error.code !== "ERR_BUFFER_OUT_OF_BOUNDS") throw error;
}

const { Buffer } = require("buffer");

if (Buffer.from(new String("test")).toString() !== "test") {
  throw new Error("boxed string conversion failed");
}
const value = { [Symbol.toPrimitive]: () => "test" };
if (Buffer.from(value).toString() !== "test") {
  throw new Error("primitive string conversion failed");
}

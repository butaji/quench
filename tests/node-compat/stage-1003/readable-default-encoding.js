const { Readable } = require("stream");
try {
  new Readable({ defaultEncoding: "invalid" });
  throw new Error("invalid default encoding was accepted");
} catch (error) {
  if (error.code !== "ERR_UNKNOWN_ENCODING") throw error;
}
const readable = new Readable({ defaultEncoding: "hex" });
if (readable.readableDefaultEncoding !== "hex") {
  throw new Error("default encoding was not retained");
}

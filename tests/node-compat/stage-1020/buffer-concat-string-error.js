const { Buffer } = require("buffer");
try {
  Buffer.concat(["hello"]);
  throw new Error("string concat item was accepted");
} catch (error) {
  if (!error.message.includes("Received type string ('hello')")) {
    throw new Error("concat string diagnostic was incorrect");
  }
}

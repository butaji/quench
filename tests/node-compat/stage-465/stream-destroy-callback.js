const { Readable } = require("stream");

const stream = new Readable();
const error = new Error("destroyed");
let callbackError;
let closed = false;
stream.on("close", () => (closed = true));
stream.destroy(error, (received) => (callbackError = received));

setTimeout(() => {
  if (!closed) throw new Error("destroy close was not emitted");
  if (callbackError !== error) {
    throw new Error("destroy callback error mismatch");
  }
  console.log("stream destroy callback passed");
}, 0);

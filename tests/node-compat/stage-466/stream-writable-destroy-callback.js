const { Writable } = require("stream");

const stream = new Writable();
const error = new Error("destroyed");
let callbackError;
let closed = false;
stream.on("close", () => (closed = true));
stream.destroy(error, (received) => (callbackError = received));

setTimeout(() => {
  if (!closed) throw new Error("writable close was not emitted");
  if (callbackError !== error) {
    throw new Error("writable destroy callback error mismatch");
  }
  console.log("stream writable destroy callback passed");
}, 0);

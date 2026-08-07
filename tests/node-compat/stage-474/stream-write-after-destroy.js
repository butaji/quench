const { Writable } = require("stream");

const stream = new Writable();
stream.destroy();
let received;
if (stream.write("late", (error) => (received = error)) !== false) {
  throw new Error("write after destroy was accepted");
}

setTimeout(() => {
  if (!received || received.code !== "ERR_STREAM_DESTROYED") {
    throw new Error("write-after-destroy error code was missing");
  }
  console.log("stream write after destroy passed");
}, 0);

const { Writable } = require("stream");

const stream = new Writable();
stream.end();
let received;
if (stream.write("late", (error) => (received = error)) !== false) {
  throw new Error("write after end was accepted");
}

setTimeout(() => {
  if (!received || received.code !== "ERR_STREAM_WRITE_AFTER_END") {
    throw new Error("write-after-end error code was missing");
  }
  console.log("stream write after end passed");
}, 0);

const { Readable } = require("stream");

const stream = new Readable();
stream.destroy();
let error;
try {
  stream.push("late");
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_STREAM_DESTROYED") {
  throw new Error("push-after-destroy error code was missing");
}

console.log("stream push after destroy passed");

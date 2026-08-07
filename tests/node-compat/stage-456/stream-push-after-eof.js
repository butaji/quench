const { Readable } = require("stream");

const stream = new Readable();
stream.push(null);
let error;
try {
  stream.push("late");
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_STREAM_PUSH_AFTER_EOF") {
  throw new Error("push-after-EOF error code was missing");
}

console.log("stream push after eof passed");

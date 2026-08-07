const { Readable } = require("stream");

const stream = new Readable();
stream.push(null);
let error;
try {
  stream.unshift("late");
} catch (caught) {
  error = caught;
}

if (!error || error.code !== "ERR_STREAM_UNSHIFT_AFTER_END_EVENT") {
  throw new Error("unshift-after-end error code was missing");
}

console.log("stream unshift after end passed");

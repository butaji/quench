const { Writable } = require("stream");

const error = new Error("kaboom");
const writable = new Writable({
  captureRejections: true,
  highWaterMark: 1,
  write(_chunk, _encoding, callback) {
    process.nextTick(callback);
  }
});

let drains = 0;
writable.write("hello", () => writable.write("world"));
writable.on("error", (actual) => {
  if (actual !== error) throw new Error("wrong rejection error");
  if (!writable.destroyed) throw new Error("writable was not destroyed");
});
writable.on("drain", async () => {
  drains++;
  throw error;
});

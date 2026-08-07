const { Writable } = require("stream");
const stream = new Writable({ highWaterMark: 4 });
const accepted = stream.write(Buffer.alloc(4), () => {});
if (accepted !== false || !stream.writableNeedDrain) {
  throw new Error("write did not expose needDrain backpressure");
}
stream.once("drain", () => {
  if (stream.writableNeedDrain) {
    throw new Error("drain did not clear needDrain");
  }
  console.log("stream writable needDrain passed");
});

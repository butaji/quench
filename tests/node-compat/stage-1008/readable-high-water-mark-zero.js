const { Readable } = require("stream");
let reads = 0;
let pushedNull = false;
const readable = new Readable({
  highWaterMark: 0,
  read() {
    reads++;
  },
});
readable.on("readable", () => {
  if (readable.read() !== null || !pushedNull || reads !== 1) {
    throw new Error("high-water-mark zero readable state mismatch");
  }
});
readable.on("end", () => {
  if (reads !== 1) throw new Error("high-water-mark zero read count mismatch");
});
process.nextTick(() => {
  if (readable.read() !== null) throw new Error("unexpected buffered data");
  pushedNull = true;
  readable.push(null);
});

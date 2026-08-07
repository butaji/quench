const { Readable, Writable } = require("stream");

const readable = Readable.from(["value"]);
if (readable.readableEnded) throw new Error("readable started ended");
readable.on("end", () => {
  if (!readable.readableEnded) throw new Error("readableEnded was not set");
});

const writable = new Writable();
if (writable.writableEnded || writable.writableFinished) {
  throw new Error("writable started finished");
}
writable.end(() => {
  if (!writable.writableEnded || !writable.writableFinished) {
    throw new Error("writable completion flags were not set");
  }
  console.log("stream completion state passed");
});

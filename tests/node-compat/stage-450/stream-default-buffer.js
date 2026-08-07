const { Readable } = require("stream");

const stream = new Readable();
stream.push(Buffer.from("queued"));

if (stream.read().toString() !== "queued") {
  throw new Error("default readable buffering failed");
}
if (stream.read() !== null) throw new Error("queue was not drained");

console.log("stream default buffer passed");

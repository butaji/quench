const { Readable } = require("stream");

const stream = new Readable();
stream.pause();
stream.push("queued");

if (stream.read(0) !== null) throw new Error("read(0) returned data");
if (stream.read().toString() !== "queued") {
  throw new Error("read(0) consumed the queue");
}

console.log("stream read zero passed");

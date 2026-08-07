const { Readable } = require("stream");

const stream = new Readable();
stream.pause();
stream.push(Buffer.from("a"));
stream.push(Buffer.from("bc"));
stream.push(Buffer.from("def"));

if (stream.read(4).toString() !== "abcd") {
  throw new Error("read did not combine queued chunks");
}
if (stream.read().toString() !== "ef") {
  throw new Error("read lost the remaining chunk");
}

console.log("stream read combine passed");

const { Readable } = require("stream");

const stream = new Readable();
stream.pause();
stream.push(Buffer.from("abcd"));
stream.push(Buffer.from("ef"));

if (stream.readableLength !== 6) {
  throw new Error("readableLength was not tracked");
}
if (stream.read(3).toString() !== "abc") throw new Error("partial read failed");
if (stream.readableLength !== 3) throw new Error("partial length was wrong");
stream.read();
stream.read();
if (stream.readableLength !== 0) {
  throw new Error("readableLength did not drain");
}

console.log("stream readable length passed");

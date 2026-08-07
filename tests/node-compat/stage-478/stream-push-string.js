const { Readable } = require("stream");

const stream = new Readable();
stream.pause();
stream.push("abcd");
if (stream.readableLength !== 4) {
  throw new Error(`string length was ${stream.readableLength}`);
}
if (stream.read(2).toString() !== "ab") {
  throw new Error("string push was not converted to bytes");
}
if (stream.read().toString() !== "cd") throw new Error("string remainder lost");

console.log("stream push string passed");

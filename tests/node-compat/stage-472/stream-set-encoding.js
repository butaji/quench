const { Readable } = require("stream");

const stream = new Readable();
stream.pause();
if (stream.setEncoding("utf8") !== stream) {
  throw new Error("setEncoding was not chainable");
}
if (stream.readableEncoding !== "utf8") {
  throw new Error("readableEncoding was not exposed");
}
stream.push(Buffer.from("hello"));
if (stream.read() !== "hello") throw new Error("encoding was not applied");

let error;
try {
  stream.setEncoding("not-an-encoding");
} catch (caught) {
  error = caught;
}
if (!error || error.code !== "ERR_UNKNOWN_ENCODING") {
  throw new Error("invalid encoding error was missing");
}

console.log("stream set encoding passed");

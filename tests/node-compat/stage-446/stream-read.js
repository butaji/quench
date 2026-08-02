const { Readable } = require("stream");
const stream = new Readable();
stream.pause();
stream.push(Buffer.from("abcd"));
stream.push(Buffer.from("ef"));

if (stream.read(2).toString() !== "ab") throw new Error("read size failed");
if (stream.read().toString() !== "cd") throw new Error("read queue failed");
if (stream.read().toString() !== "ef") throw new Error("read FIFO failed");
if (stream.read() !== null) throw new Error("empty read should return null");

console.log("stream read passed");

const { Writable } = require("stream");
const stream = new Writable();

if (stream.writableCorked !== 0) throw new Error("writable started corked");
stream.cork();
stream.cork();
if (stream.writableCorked !== 2) {
  throw new Error("cork nesting was not tracked");
}
stream.uncork();
stream.uncork();
stream.uncork();
if (stream.writableCorked !== 0) throw new Error("uncork did not unwind state");

console.log("stream cork passed");

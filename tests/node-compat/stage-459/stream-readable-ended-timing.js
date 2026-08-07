const { Readable } = require("stream");

const stream = new Readable();
let dataEnded;
let endEnded;
stream.on("data", () => (dataEnded = stream.readableEnded));
stream.on("end", () => (endEnded = stream.readableEnded));
stream.push("data");
stream.push(null);

if (dataEnded !== false) throw new Error("readableEnded changed during data");
if (stream.readableEnded !== false) {
  throw new Error("readableEnded changed before end");
}

setTimeout(() => {
  if (endEnded !== true || !stream.readableEnded) {
    throw new Error("readableEnded was not set at end");
  }
  console.log("stream readable ended timing passed");
}, 0);

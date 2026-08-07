const { Readable } = require("stream");

const stream = new Readable();
const events = [];
stream.on("end", () => events.push("end"));
stream.pause();
stream.push("body");
stream.unshift(null);

if (events.length !== 0) throw new Error("unshift EOF emitted early");
if (stream.read().toString() !== "body") {
  throw new Error("buffered body was lost");
}
if (events.join(",") !== "end") throw new Error("unshift EOF failed");

console.log("stream unshift eof passed");

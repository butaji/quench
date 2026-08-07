const { Readable } = require("stream");

const stream = new Readable();
const events = [];
stream.on("data", () => events.push("data"));
stream.on("end", () => events.push("end"));
stream.pause();
stream.push(Buffer.from("payload"));
stream.push(null);

if (events.length !== 0) throw new Error("paused stream emitted early");
if (stream.read().toString() !== "payload") {
  throw new Error("buffered data was not readable");
}
stream.resume();
if (events.join(",") !== "end") throw new Error("end ordering failed");

console.log("stream eof order passed");

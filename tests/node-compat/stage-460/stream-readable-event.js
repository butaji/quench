const { Readable } = require("stream");

const stream = new Readable();
const events = [];
stream.pause();
stream.on("readable", () => {
  events.push(stream.read().toString());
});
stream.push("queued");

setTimeout(() => {
  if (events.join(",") !== "queued") {
    throw new Error("readable event did not expose queued data");
  }
  console.log("stream readable event passed");
}, 0);

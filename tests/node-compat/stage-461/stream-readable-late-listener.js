const { Readable } = require("stream");

const stream = new Readable();
stream.pause();
stream.push("queued");
let value;
stream.on("readable", () => (value = stream.read().toString()));

setTimeout(() => {
  if (value !== "queued") throw new Error("late readable listener was missed");
  console.log("stream readable late listener passed");
}, 0);

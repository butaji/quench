const { Readable } = require("stream");

const stream = new Readable();
const values = [];
stream.on("data", (value) => values.push(value));
stream.pause();
stream.push("one");
stream.push("two");
stream.resume();

setTimeout(() => {
  if (values.join(",") !== "one,two") {
    throw new Error("resume did not drain queued data");
  }
  console.log("stream resume queue passed");
}, 0);

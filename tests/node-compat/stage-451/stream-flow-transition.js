const { Readable } = require("stream");

const stream = new Readable();
const events = [];
stream.push("queued");
stream.push(null);
stream.on("data", (chunk) => events.push(`data:${chunk}`));
stream.on("end", () => events.push("end"));

setTimeout(() => {
  if (events.join(",") !== "data:queued,end") {
    throw new Error("flow transition did not drain buffered data");
  }
  console.log("stream flow transition passed");
}, 0);

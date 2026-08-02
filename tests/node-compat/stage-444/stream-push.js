const { Readable } = require("stream");
const stream = new Readable();
const chunks = [];
stream.on("data", (chunk) => chunks.push(chunk));
let ended = false;
stream.on("end", () => {
  ended = true;
});

if (!stream.push("one") || !stream.push("two"))
  throw new Error("push did not accept chunks");
if (stream.push(null) !== false || !ended)
  throw new Error("push(null) did not end the stream");
if (chunks.join("") !== "onetwo") throw new Error("push lost chunks");

console.log("stream push passed");

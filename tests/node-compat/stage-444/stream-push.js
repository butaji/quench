const { Readable } = require("stream");
const stream = new Readable();
const chunks = [];
stream.on("data", (chunk) => chunks.push(chunk));
let ended = false;
stream.on("end", () => {
  ended = true;
});

if (!stream.push("one") || !stream.push("two")) {
  throw new Error("push did not accept chunks");
}
if (stream.push(null) !== false) {
  throw new Error("push(null) did not return false");
}
queueMicrotask(() => {
  if (!ended) throw new Error("stream did not emit end");
});
if (chunks.join("") !== "onetwo") throw new Error("push lost chunks");

console.log("stream push passed");

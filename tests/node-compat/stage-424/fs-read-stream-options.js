const fs = require("fs");
const path = `/tmp/quench-node-stage-424-${process.pid}`;
fs.writeFileSync(path, "stream text");

const chunks = [];
const stream = fs.createReadStream(path, { encoding: "utf8" });
stream.on("data", (chunk) => {
  if (typeof chunk !== "string") throw new Error("encoding was ignored");
  chunks.push(chunk);
});
stream.on("close", () => {
  if (chunks.join("") !== "stream text") throw new Error("wrong stream text");
  if (stream.bytesRead !== 11) throw new Error("bytesRead was not updated");
  fs.unlinkSync(path);
  console.log("fs read stream options passed");
});

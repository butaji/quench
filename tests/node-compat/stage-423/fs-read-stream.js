const fs = require("fs");
const path = `/tmp/quench-node-stage-423-${process.pid}`;
fs.writeFileSync(path, "read stream data");

const chunks = [];
const stream = fs.createReadStream(path);
stream.on("data", (chunk) => chunks.push(chunk));
stream.on("close", () => {
  if (Buffer.concat(chunks).toString() !== "read stream data") {
    throw new Error("read stream content was incorrect");
  }
  fs.unlinkSync(path);
  console.log("fs read stream passed");
});

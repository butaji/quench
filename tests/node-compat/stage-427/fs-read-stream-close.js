const fs = require("fs");
const path = `/tmp/quench-node-stage-427-${process.pid}`;
fs.writeFileSync(path, "close me");

const stream = fs.createReadStream(path);
stream.on("close", () => {
  if (stream.fd !== null) throw new Error("read stream fd was not closed");
  fs.unlinkSync(path);
  console.log("fs read stream close passed");
});

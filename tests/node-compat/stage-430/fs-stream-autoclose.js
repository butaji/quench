const fs = require("fs");
const path = `/tmp/quench-node-stage-430-${process.pid}`;
fs.writeFileSync(path, "keep fd");

const read = fs.createReadStream(path, { autoClose: false });
read.on("close", () => {
  if (read.fd === null) throw new Error("read stream ignored autoClose false");
  fs.closeSync(read.fd);
  fs.unlinkSync(path);
  console.log("fs stream autoclose passed");
});

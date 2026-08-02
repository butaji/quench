const fs = require("fs");
const path = `/tmp/quench-node-stage-390-${process.pid}`;

fs.writeFileSync(path, "data");
const fd = fs.openSync(path, "r");
fs.close(fd, (error) => {
  if (error) throw error;
  let closeError;
  try {
    fs.closeSync(fd);
  } catch (caught) {
    closeError = caught;
  }
  if (!closeError || closeError.code !== "EBADF") {
    throw new Error("callback close must release the descriptor");
  }
  fs.unlinkSync(path);
  console.log("fs close callback passed");
});

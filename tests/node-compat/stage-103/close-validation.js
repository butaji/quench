const fs = require("fs");

const path = "/tmp/quench-node-stage-103";
const fd = fs.openSync(path, "w");
fs.close(fd, (error) => {
  if (error) throw error;
  try {
    fs.closeSync(fd);
    throw new Error("closed fd accepted");
  } catch (closeError) {
    if (closeError.code !== "EBADF") throw closeError;
  }
  fs.rmSync(path);
});

try {
  fs.closeSync("fd");
  throw new Error("invalid fd accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}
try {
  fs.close(1);
  throw new Error("missing callback accepted");
} catch (error) {
  if (error.code !== "ERR_INVALID_ARG_TYPE") throw error;
}

const fs = require("fs");
const common = require("../common");

const path = "/tmp/quench-node-stage-70-file";
const fd = fs.openSync(path, "w");
fs.closeSync(fd);
fs.open(
  path,
  "r",
  common.mustCall((error, asyncFd) => {
    if (error) throw error;
    fs.closeSync(asyncFd);
  }),
);
fs.promises
  .open(path)
  .then((handle) => handle.close())
  .catch((error) => {
    throw error;
  });

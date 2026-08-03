const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-121-${process.pid}`;
  const fd = fs.openSync(path, "w+");
  const buffers = [Buffer.from("ab"), Buffer.from("cd")];
  const result = await fs.promises.writev(fd, buffers, 0);
  fs.closeSync(fd);
  if (result.bytesWritten !== 4 || fs.readFileSync(path, "utf8") !== "abcd")
    throw new Error("promise writev mismatch");
})().then(() => undefined);

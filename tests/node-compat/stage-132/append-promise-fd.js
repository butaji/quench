const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-132-${process.pid}`;
  fs.writeFileSync(path, "a");
  const fd = fs.openSync(path, "a");
  await fs.promises.appendFile(fd, "b");
  fs.closeSync(fd);
  if (fs.readFileSync(path, "utf8") !== "ab")
    throw new Error("promise append fd mismatch");
})().then(() => undefined);

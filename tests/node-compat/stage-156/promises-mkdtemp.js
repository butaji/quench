const fs = require("fs");

(async () => {
  const prefix = `/tmp/quench-node-stage-156-${process.pid}-`;
  const path = await fs.promises.mkdtemp(prefix);
  if (!path.startsWith(prefix) || !fs.statSync(path).isDirectory())
    throw new Error("promise mkdtemp mismatch");
  fs.rmdirSync(path);
})().then(() => undefined);

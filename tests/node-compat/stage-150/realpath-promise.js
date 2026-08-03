const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-150-${process.pid}`;
  fs.writeFileSync(path, "x");
  if ((await fs.promises.realpath(path)) !== fs.realpathSync(path))
    throw new Error("promise realpath mismatch");
  const result = await fs.promises.realpath(path, { encoding: "buffer" });
  if (!Buffer.isBuffer(result) || result.toString() !== fs.realpathSync(path))
    throw new Error("promise realpath buffer mismatch");
})().then(() => undefined);

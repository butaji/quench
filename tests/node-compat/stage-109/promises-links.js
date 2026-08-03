const fs = require("fs");

(async () => {
  const target = `/tmp/quench-node-stage-109-target-${process.pid}`;
  const link = `/tmp/quench-node-stage-109-link-${process.pid}`;
  fs.writeFileSync(target, "x");
  await fs.promises.symlink(target, link);
  if ((await fs.promises.readlink(link)) !== target)
    throw new Error("promise link mismatch");
})().then(() => undefined);

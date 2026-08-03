const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-153-${process.pid}`;
  fs.writeFileSync(path, "mode");
  await fs.promises.chmod(path, 0o600);
  if ((fs.statSync(path).mode & 0o777) !== 0o600)
    throw new Error("promise chmod mismatch");
})().then(() => undefined);

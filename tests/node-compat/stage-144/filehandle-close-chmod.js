const fs = require("fs");

(async () => {
  const path = `/tmp/quench-node-stage-144-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  await handle.chmod(0o600);
  if ((fs.statSync(path).mode & 0o777) !== 0o600)
    throw new Error("filehandle chmod mismatch");
  await handle.close();
  try {
    await handle.stat();
    throw new Error("closed handle remained usable");
  } catch (error) {
    if (error.code !== "EBADF") throw error;
  }
})().then(() => undefined);

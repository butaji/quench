const fs = require("fs");

(async () => {
  const root = `/tmp/quench-node-stage-154-${process.pid}`;
  const source = `${root}-source`;
  const copy = `${root}-copy`;
  const renamed = `${root}-renamed`;
  fs.writeFileSync(source, "copy");
  await fs.promises.copyFile(source, copy);
  await fs.promises.rename(copy, renamed);
  if (fs.readFileSync(renamed, "utf8") !== "copy")
    throw new Error("promise mutation mismatch");
  await fs.promises.unlink(renamed);
})().then(() => undefined);

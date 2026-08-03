const fs = require("fs");
const assert = require("assert");

(async () => {
  const root = `/tmp/quench-node-stage-154-${process.pid}`;
  const source = `${root}-source`;
  const copy = `${root}-copy`;
  const renamed = `${root}-renamed`;
  fs.writeFileSync(source, "copy");
  await fs.promises.copyFile(source, copy);
  await fs.promises.rename(copy, renamed);
  assert.strictEqual(fs.readFileSync(renamed, "utf8"), "copy");
  await fs.promises.unlink(renamed);
  await fs.promises.unlink(source);
})().then(() => undefined);

const fs = require("fs");
const assert = require("assert");

(async () => {
  const source = `/tmp/quench-node-stage-158-source-${process.pid}`;
  const target = `/tmp/quench-node-stage-158-target-${process.pid}`;
  fs.writeFileSync(source, "hard");
  fs.linkSync(source, target);
  assert.strictEqual(fs.readFileSync(target, "utf8"), "hard");
  await fs.promises.unlink(target);
  await new Promise((resolve, reject) =>
    fs.link(source, target, (error) => (error ? reject(error) : resolve()))
  );
  await fs.promises.unlink(target);
  fs.unlinkSync(source);
})().then(() => undefined);

const fs = require("fs");
const assert = require("assert");

(async () => {
  const target = `/tmp/quench-node-stage-160-target-${process.pid}`;
  const link = `/tmp/quench-node-stage-160-link-${process.pid}`;
  fs.writeFileSync(target, "x");
  fs.symlinkSync(target, link);
  const expected = fs.readlinkSync(link);
  assert.strictEqual(fs.readlinkSync(link, "utf8"), expected);
  assert.strictEqual(fs.readlinkSync(link, "buffer").toString(), expected);
  fs.unlinkSync(link);
  fs.rmSync(target);
})().then(() => undefined);

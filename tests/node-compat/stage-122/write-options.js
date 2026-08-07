const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-122-${process.pid}`;
  const fd = fs.openSync(path, "w+");
  const buffer = Buffer.from("abcd");
  const written = await new Promise((resolve, reject) =>
    fs.write(
      fd,
      { buffer, offset: 1, length: 2, position: 0 },
      (error, count) => (error ? reject(error) : resolve(count)),
    )
  );
  fs.closeSync(fd);
  assert.strictEqual(written, 2);
  assert.strictEqual(fs.readFileSync(path, "utf8"), "bc");
  fs.rmSync(path);
})().then(() => undefined);

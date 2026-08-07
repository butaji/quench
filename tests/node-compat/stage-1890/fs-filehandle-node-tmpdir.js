const assert = require("assert");
const fs = require("fs");
const path = require("path");
const tmpdir = require("../../node/test/common/tmpdir");

(async () => {
  tmpdir.refresh();
  const file = path.resolve(tmpdir.path, "read-file");
  const data = Buffer.from("Hello world");
  const fd = fs.openSync(file, "w+");
  fs.writeSync(fd, data, 0, data.length);
  fs.closeSync(fd);
  const handle = await fs.promises.open(file, "w+");
  let closed = 0;
  handle.on("close", () => closed++);
  const result = await handle.read(Buffer.alloc(11), 0, 11, 0);
  assert.strictEqual(result.bytesRead, 11);
  await handle.close();
  assert.strictEqual(closed, 1);
})();

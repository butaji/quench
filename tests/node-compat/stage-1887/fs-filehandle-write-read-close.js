const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const file = path.join("/tmp", `quench-filehandle-${process.pid}`);
  fs.writeFileSync(file, "Hello world");
  const handle = await fs.promises.open(file, "w+");
  let closed = 0;
  handle.once("close", () => closed++);
  const buffer = Buffer.alloc(11);
  const result = await handle.read(buffer, 0, 11, 0);
  assert.strictEqual(result.bytesRead, 11);
  assert.strictEqual(result.buffer.toString(), "Hello world");
  await handle.close();
  assert.strictEqual(closed, 1);
})();

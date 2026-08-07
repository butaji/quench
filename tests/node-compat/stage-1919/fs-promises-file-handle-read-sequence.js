const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const file = path.join(
    process.cwd(),
    "target",
    "compat",
    "stage-1919-read.txt",
  );
  fs.writeFileSync(file, "Hello world");
  const handle = await fs.promises.open(file, "r");
  let closed = 0;
  handle.on("close", () => closed++);
  const buffer = Buffer.alloc(11);
  const first = await handle.read(buffer, 0, 11, 0);
  assert.strictEqual(first.bytesRead, 11);
  assert.strictEqual(first.buffer, buffer);
  assert.strictEqual(buffer.toString(), "Hello world");
  const second = await handle.read({
    buffer: Buffer.alloc(5),
    offset: 0,
    length: 5,
    position: 6,
  });
  assert.strictEqual(second.buffer.toString(), "world");
  await handle.close();
  assert.strictEqual(closed, 1);
  fs.unlinkSync(file);
  console.log("fs FileHandle read sequence passed");
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

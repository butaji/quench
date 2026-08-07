const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const file = path.join(
    process.cwd(),
    "target",
    "compat",
    "stage-1921-read.txt",
  );
  const fd = fs.openSync(file, "w+");
  const handle = await fs.promises.open(file, "w+");
  const streamHandle = await fs.promises.open(file, "w+");
  fs.writeSync(fd, Buffer.from("Hello world"), 0, 11);
  fs.closeSync(fd);
  let closed = 0;
  handle.on("close", () => closed++);
  const result = await handle.read(Buffer.alloc(11), 0, 11, 0);
  assert.strictEqual(result.bytesRead, 11);
  await handle.close();
  const stream = fs.createReadStream(null, { fd: streamHandle });
  let value = Buffer.alloc(0);
  for await (const chunk of stream) value = Buffer.from(chunk);
  assert.strictEqual(value.toString(), "Hello world");
  await streamHandle.close();
  assert.strictEqual(closed, 1);
  fs.unlinkSync(file);
  console.log("fs FileHandle open-before-write passed");
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

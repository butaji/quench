const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const file = path.join(
    process.cwd(),
    "target",
    "compat",
    "stage-1920-read.txt",
  );
  fs.writeFileSync(file, "Hello world");
  const handle = await fs.promises.open(file, "r");
  let closed = 0;
  handle.on("close", () => closed++);
  const stream = fs.createReadStream(null, { fd: handle });
  let value = Buffer.alloc(0);
  for await (const chunk of stream) value = Buffer.from(chunk);
  assert.strictEqual(value.toString(), "Hello world");
  await handle.close();
  assert.strictEqual(closed, 1);
  fs.unlinkSync(file);
  console.log("fs FileHandle read stream passed");
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

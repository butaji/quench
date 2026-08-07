const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const file = path.join(
    process.cwd(),
    "target",
    "compat",
    "stage-1918-read.txt",
  );
  fs.writeFileSync(file, "xyz");
  const handle = await fs.promises.open(file, "r");
  const buffer = Buffer.alloc(2);
  const result = await handle.read({
    buffer,
    offset: 0,
    length: 2,
    position: 1,
  });
  assert.strictEqual(result.bytesRead, 2);
  assert.strictEqual(result.buffer, buffer);
  assert.strictEqual(buffer.toString(), "yz");
  await handle.close();
  fs.unlinkSync(file);
  console.log("fs promises FileHandle.read passed");
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

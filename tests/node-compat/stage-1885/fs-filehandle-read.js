const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const file = path.join(process.cwd(), "tests/node/fixtures/x.txt");
  const handle = await fs.promises.open(file, "r");
  const buffer = Buffer.alloc(3);
  const result = await handle.read({
    buffer,
    offset: 0,
    length: 3,
    position: 0,
  });
  assert.strictEqual(result.bytesRead, 3);
  assert.strictEqual(result.buffer.toString(), "xyz");
  const second = Buffer.alloc(1);
  assert.strictEqual(
    (await handle.read(second, 0, 1, 1)).buffer.toString(),
    "y",
  );
  await handle.close();
})();

const assert = require("assert");
const fs = require("fs");
const path = require("path");

(async () => {
  const handle = await fs.promises.open(
    path.join(process.cwd(), "tests/node/fixtures/x.txt"),
    "r",
  );
  let closed = 0;
  handle.once("close", () => closed++);
  await handle.close();
  assert.strictEqual(closed, 1);
})();

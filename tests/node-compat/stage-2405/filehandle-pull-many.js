const assert = require("assert");
const fs = require("fs");
const path = require("path");
const { text } = require("stream/iter");

const root = fs.mkdtempSync(path.join(process.cwd(), "pull-many-"));
const jobs = Array.from({ length: 19 }, async (_, index) => {
  const filename = path.join(root, `file-${index}`);
  fs.writeFileSync(filename, `value-${index}`);
  const handle = await fs.promises.open(filename, "r");
  try {
    assert.strictEqual(await text(handle.pull()), `value-${index}`);
  } finally {
    await handle.close();
  }
});

Promise.all(jobs).then(() => console.log("many concurrent pulls passed"));

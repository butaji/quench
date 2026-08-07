const assert = require("assert");
const fs = require("fs");
const path = require("path");

const root = path.join(process.cwd(), "tests/node-compat");

(async () => {
  const dir = await fs.promises.opendir(root);
  const first = await dir.read();
  assert(first instanceof fs.Dirent);
  await dir.close();
  await assert.rejects(dir.close(), { code: "ERR_DIR_CLOSED" });

  const iterDir = await fs.promises.opendir(root);
  let count = 0;
  for await (const entry of iterDir) {
    assert(entry instanceof fs.Dirent);
    count++;
  }
  assert(count > 0);
  await assert.rejects(iterDir.read(), { code: "ERR_DIR_CLOSED" });

  const concurrent = await fs.promises.opendir(root);
  const read1 = concurrent.read();
  const read2 = concurrent.read();
  assert((await read1) instanceof fs.Dirent);
  assert((await read2) instanceof fs.Dirent);
  concurrent.closeSync();

  const mixed = await fs.promises.opendir(root);
  const read = mixed.read();
  const close = mixed.close();
  assert((await read) instanceof fs.Dirent);
  await close;
})();

const assert = require("assert");
const fs = require("fs");
const path = require("path");
const { text } = require("stream/iter");

const root = fs.mkdtempSync(path.join(process.cwd(), "pull-lifecycle-"));
const make = (name, value) => {
  const file = path.join(root, name);
  fs.writeFileSync(file, value);
  return file;
};

Promise.all([
  (async () => {
    const fh = await fs.promises.open(make("auto", "auto close"), "r");
    const data = await text(fh.pull({ autoClose: true }));
    assert.strictEqual(data, "auto close");
    await assert.rejects(
      fh.stat(),
      (error) => error.code === "ERR_INVALID_STATE" || error.code === "EBADF",
    );
  })(),
  (async () => {
    const fh = await fs.promises.open(make("transform", "hello"), "r");
    try {
      const upper = (chunks) =>
        chunks &&
        chunks.map((chunk) => Buffer.from(chunk).toString().toUpperCase());
      assert.strictEqual(await text(fh.pull(upper)), "HELLO");
    } finally {
      await fh.close();
    }
  })(),
  (async () => {
    const fh = await fs.promises.open(make("empty", ""), "r");
    try {
      assert.strictEqual(await text(fh.pull()), "");
    } finally {
      await fh.close();
    }
  })(),
  (async () => {
    const fh = await fs.promises.open(make("lock", "lock"), "r");
    try {
      const first = fh.pull();
      assert.throws(() => fh.pull(), { code: "ERR_INVALID_STATE" });
      assert.strictEqual(await text(first), "lock");
      assert.strictEqual(await text(fh.pull()), "");
    } finally {
      await fh.close();
    }
  })(),
]).then(() => console.log("pull lifecycle contracts passed"));

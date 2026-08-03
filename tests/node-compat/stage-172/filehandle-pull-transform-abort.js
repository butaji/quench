const fs = require("fs");
const assert = require("assert");
const { text } = require("stream/iter");

(async () => {
  const path = `/tmp/quench-node-stage-172-${process.pid}`;
  fs.writeFileSync(path, "hello");
  const handle = await fs.promises.open(path, "r");
  const upper = (chunks) =>
    chunks.map((chunk) =>
      new TextEncoder().encode(new TextDecoder().decode(chunk).toUpperCase())
    );
  assert.strictEqual(await text(handle.pull(upper)), "HELLO");
  await handle.close();
  const aborted = await fs.promises.open(path, "r");
  const controller = new AbortController();
  controller.abort();
  try {
    await text(aborted.pull({ signal: controller.signal }));
    assert.fail("pull abort accepted");
  } catch (error) {
    assert.strictEqual(error.name, "AbortError");
  }
  await aborted.close();
  fs.rmSync(path);
})();

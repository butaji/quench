const fs = require("fs");
const { text } = require("stream/iter");

(async () => {
  const path = `/tmp/quench-node-stage-172-${process.pid}`;
  fs.writeFileSync(path, "hello");
  const handle = await fs.promises.open(path, "r");
  const upper = (chunks) =>
    chunks.map((chunk) =>
      new TextEncoder().encode(new TextDecoder().decode(chunk).toUpperCase())
    );
  if ((await text(handle.pull(upper))) !== "HELLO")
    throw new Error("pull transform mismatch");
  await handle.close();
  const aborted = await fs.promises.open(path, "r");
  const controller = new AbortController();
  controller.abort();
  try {
    await text(aborted.pull({ signal: controller.signal }));
    throw new Error("pull abort accepted");
  } catch (error) {
    if (error.name !== "AbortError") throw error;
  }
  await aborted.close();
})();

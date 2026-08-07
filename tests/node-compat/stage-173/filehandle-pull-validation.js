const fs = require("fs");
const assert = require("assert");

(async () => {
  const path = `/tmp/quench-node-stage-173-${process.pid}`;
  const handle = await fs.promises.open(path, "w+");
  for (
    const options of [
      { autoClose: "no" },
      { signal: {} },
      { start: "a" },
      { limit: 1.1 },
      { chunkSize: 1.1 },
    ]
  ) {
    try {
      handle.pull(options);
      assert.fail("accepted invalid pull option");
    } catch (error) {
      assert.strictEqual(
        ["ERR_INVALID_ARG_TYPE", "ERR_OUT_OF_RANGE"].includes(error.code),
        true,
      );
    }
  }
  await handle.close();
  fs.rmSync(path);
})();

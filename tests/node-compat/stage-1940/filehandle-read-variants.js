const assert = require("assert");
const fs = require("fs");
const path = "/tmp/quench-node-stage-1940";
fs.writeFileSync(path, "xyz");

const hold = setInterval(() => {}, 1000);
(async () => {
  const handle = await fs.promises.open(path, "r");
  try {
    for (
      const [label, call] of [
        ["no-params", () => handle.read()],
        ["position-zero", () => handle.read(Buffer.alloc(1), 0, 1, 0)],
        ["null-length", () => handle.read(Buffer.alloc(1), 0, null, 0)],
        [
          "undefined-length",
          () => handle.read(Buffer.alloc(1), 0, undefined, 0),
        ],
        ["options-null", () =>
          handle.read({
            buffer: Buffer.alloc(1),
            offset: 0,
            length: null,
            position: 0,
          })],
        ["options-undefined", () =>
          handle.read({
            buffer: Buffer.alloc(1),
            offset: 0,
            length: undefined,
            position: 0,
          })],
      ]
    ) {
      const result = await call();
      assert.strictEqual(
        result.bytesRead,
        label === "no-params" ? 3 : 1,
        label,
      );
      console.log(`${label}: ok`);
    }
  } finally {
    await handle.close();
    fs.unlinkSync(path);
  }
})().then(() => {
  clearInterval(hold);
  console.log("filehandle read variants passed");
});

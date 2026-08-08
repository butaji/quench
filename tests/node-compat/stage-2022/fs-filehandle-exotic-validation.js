const assert = require("assert");
const fs = require("fs");

(async () => {
  const path = "stage-2022-write.txt";
  const handle = await fs.promises.open(path, "w+");
  const invalid = [
    undefined,
    null,
    true,
    42,
    42n,
    Symbol("42"),
    NaN,
    [],
    () => {},
    {},
    { buffer: "amNotParam" },
    { string: "amNotParam" },
    { buffer: new Uint8Array(1).buffer },
    new Date(),
    new String("notPrimitive"),
    {
      toString() {
        return "amObject";
      }
    },
    { [Symbol.toPrimitive]: () => "amObject" },
    Promise.resolve(new Uint8Array(1))
  ];
  for (const value of invalid) {
    await assert.rejects(handle.write(value, {}), {
      code: "ERR_INVALID_ARG_TYPE"
    });
  }
  await handle.close();
  fs.unlinkSync(path);
  console.log("exotic filehandle validation passed");
})().catch((error) => {
  throw error;
});

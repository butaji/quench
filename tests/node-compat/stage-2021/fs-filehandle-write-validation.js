const assert = require("assert");
const fs = require("fs");

(async () => {
  const path = "stage-2021-write.txt";
  const handle = await fs.promises.open(path, "w+");
  const invalid = [undefined, null, true, 42, 42n, Symbol("x"), NaN, [], {}, {
    buffer: "bad",
  }, new Date()];
  for (const value of invalid) {
    await assert.rejects(handle.write(value, {}), {
      code: "ERR_INVALID_ARG_TYPE",
    });
  }
  await assert.rejects(handle.write(Buffer.from("zyx"), { length: 5 }), {
    code: "ERR_OUT_OF_RANGE",
  });
  await assert.rejects(handle.write(Buffer.from("zyx"), { offset: 5 }), {
    code: "ERR_OUT_OF_RANGE",
  });
  await assert.rejects(handle.write(Buffer.from("zyx"), { offset: false }), {
    code: "ERR_INVALID_ARG_TYPE",
  });
  await handle.close();
  fs.unlinkSync(path);
  console.log("filehandle write validation passed");
})().catch((error) => {
  throw error;
});

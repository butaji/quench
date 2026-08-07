const { randomFillSync } = require("crypto");

for (
  const call of [
    () => randomFillSync("not-a-buffer"),
    () => randomFillSync(Buffer.alloc(2), 1, 2),
  ]
) {
  let error;
  try {
    call();
  } catch (caught) {
    error = caught;
  }
  if (
    !error ||
    !["ERR_INVALID_ARG_TYPE", "ERR_OUT_OF_RANGE"].includes(error.code)
  ) {
    throw new Error("randomFillSync validation was missing");
  }
}

console.log("crypto random fill validation passed");

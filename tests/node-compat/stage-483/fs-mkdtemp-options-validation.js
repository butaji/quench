const fs = require("fs");

for (
  const call of [
    () => fs.mkdtempSync("/tmp/quench-node-", 123),
    () => fs.mkdtemp("/tmp/quench-node-", 123, () => {}),
  ]
) {
  let error;
  try {
    call();
  } catch (caught) {
    error = caught;
  }
  if (!error || error.code !== "ERR_INVALID_ARG_TYPE") {
    throw new Error("mkdtemp options validation was missing");
  }
}

console.log("fs mkdtemp options validation passed");

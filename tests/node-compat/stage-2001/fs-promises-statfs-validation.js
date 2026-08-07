const assert = require("assert");
const fs = require("fs/promises");

(async () => {
  await assert.rejects(fs.statfs(), { code: "ERR_INVALID_ARG_TYPE" });
  console.log("fs promises statfs validation passed");
})().catch((error) => {
  console.error(error);
  throw error;
});

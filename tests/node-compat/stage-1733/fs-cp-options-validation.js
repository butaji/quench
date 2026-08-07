const assert = require("node:assert");
const fs = require("node:fs");

assert.throws(() => fs.cp("a", "b", "invalid", () => {}), {
  code: "ERR_INVALID_ARG_TYPE",
});
assert
  .rejects(
    fs.promises.cp("a", "b", () => {}),
    { code: "ERR_INVALID_ARG_TYPE" },
  )
  .then(() => console.log("fs cp options validation passed"));

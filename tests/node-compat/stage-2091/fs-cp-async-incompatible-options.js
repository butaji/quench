const assert = require("assert");
const fs = require("fs");

assert.throws(
  () =>
    fs.cp(
      "source",
      "destination",
      {
        dereference: true,
        verbatimSymlinks: true
      },
      () => {}
    ),
  { code: "ERR_INCOMPATIBLE_OPTION_PAIR" }
);

assert.rejects(
  fs.promises.cp("source", "destination", {
    dereference: true,
    verbatimSymlinks: true
  }),
  { code: "ERR_INCOMPATIBLE_OPTION_PAIR" }
);

const util = require("util");
const result = util.formatWithOptions(
  { colors: true },
  true,
  undefined,
  Symbol(),
  1,
  5n,
  null,
  "foobar",
);
if (!result.includes("\u001b[33mtrue\u001b[39m")) throw new Error(result);

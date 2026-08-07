const util = require("util");
const result = util.formatWithOptions({ colors: true, compact: 3 }, "%s", [
  1,
  { a: true },
]);
if (result !== "[ 1, [Object] ]") throw new Error(result);

const util = require("util");

if (
  !util.inspect.defaultOptions ||
  util.inspect.defaultOptions.numericSeparator !== false
) {
  throw new Error("inspect options missing");
}
if (util.formatWithOptions({}, "value") !== "value") {
  throw new Error("formatWithOptions mismatch");
}

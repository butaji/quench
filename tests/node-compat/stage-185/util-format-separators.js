const util = require("util");

const previous = util.inspect.defaultOptions.numericSeparator;
util.inspect.defaultOptions.numericSeparator = true;
if (
  util.format("%d %s %i", 123456789, 123456789, 123456789) !==
    "123_456_789 123_456_789 123_456_789"
) {
  throw new Error("numeric separator mismatch");
}
if (
  util.format("%d %s", 1180591620717411303424n, 12345678901234567890123n) !==
    "1_180_591_620_717_411_303_424n 12_345_678_901_234_567_890_123n"
) {
  throw new Error("large BigInt separator mismatch");
}
util.inspect.defaultOptions.numericSeparator = previous;
if (
  util.formatWithOptions({ numericSeparator: true }, "%d", 123456789) !==
    "123_456_789"
) {
  throw new Error("formatWithOptions separator mismatch");
}
if (
  util.formatWithOptions(
    { numericSeparator: true },
    "%d %i",
    123456789n,
    987654321n,
  ) !== "123_456_789n 987_654_321n"
) {
  throw new Error("BigInt separator mismatch");
}

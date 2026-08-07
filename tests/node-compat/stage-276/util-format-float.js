const { format } = require("util");

const actual = format("%f", Symbol("foo"));
if (actual !== "NaN") throw new Error("symbol float format mismatch");
if (format("%f", "") !== "NaN") throw new Error("empty float mismatch");
if (format("%d", " -0.000") !== "-0") {
  throw new Error("negative zero integer mismatch");
}
if (format("%s", -0) !== "-0") throw new Error("negative zero string mismatch");

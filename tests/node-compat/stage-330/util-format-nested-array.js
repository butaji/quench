const util = require("util");
const value = { func: [{ a: function a() {} }] };
const output = util.format("%o", value);
if (
  !output.includes("[length]: 1") ||
  !output.includes("<ref *1> [Function: a]")
) {
  throw new Error(output);
}

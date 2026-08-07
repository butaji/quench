const util = require("util");
const value = { foo: "bar", foobar: { foo: "bar", func: function func() {} } };
const output = util.format("%o", value);
if (
  !output.includes("foobar: {") ||
  !output.includes("<ref *1> [Function: func]")
) {
  throw new Error(output);
}

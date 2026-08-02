const util = require("util");
const value = { foo: "bar", foobar: 1, func: function () {} };
const output = util.format("%o", value);
if (!output.includes("[Function: func]")) throw new Error(output);

const util = require("util");
const value = { foo: "bar", func: function func() {} };
if (util.format("%O", value) !== "{ foo: 'bar', func: [Function: func] }") {
  throw new Error(util.format("%O", value));
}
if (util.format("%O", "foo") !== "'foo'") {
  throw new Error(util.format("%O", "foo"));
}

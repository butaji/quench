const util = require("util");
const value = { foo: "bar", foobar: 1, func: function () {} };
const output = util.format("%o", value);
if (
  output !==
    "{\n  foo: 'bar',\n  foobar: 1,\n  func: <ref *1> [Function: func] {\n    [length]: 0,\n    [name]: 'func',\n    [prototype]: { [constructor]: [Circular *1] }\n  }\n}"
) {
  throw new Error(output);
}

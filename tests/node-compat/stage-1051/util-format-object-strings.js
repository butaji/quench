const assert = require("assert");
const util = require("util");

assert.strictEqual(
  util.format("%O", { foo: "bar", count: 1 }),
  "{ foo: 'bar', count: 1 }",
);

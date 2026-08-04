const assert = require("assert");
const { test } = require("node:test");

test("assert context", (context) => {
  assert.strictEqual(context.assert, assert);
});

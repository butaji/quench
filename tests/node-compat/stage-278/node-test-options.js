const { test } = require("node:test");

let called = false;
test("with options", { skip: false }, () => {
  called = true;
});
if (!called) throw new Error("node:test options callback did not run");

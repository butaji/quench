const test = require("node:test");

let hooked = 0;
test.describe("scoped hooks", () => {
  test.beforeEach(() => {
    hooked++;
  });
  test("inside", () => {});
});
test("outside", () => {
  if (hooked !== 1) throw new Error(`hook leaked outside describe: ${hooked}`);
});

test.run().then((summary) => {
  if (summary.fail !== 0 || summary.pass !== 2) {
    throw new Error(`unexpected node:test summary: ${JSON.stringify(summary)}`);
  }
});

const assert = require("node:assert");
const { URLPattern } = require("node:url");

assert.throws(
  () =>
    new URLPattern(
      {},
      {
        get ignoreCase() {
          throw new Error("boom");
        },
      },
    ),
  { message: "boom" },
);

const result = new URLPattern({ pathname: "/:value" }).exec(
  "https://example.com/test",
);
assert.deepStrictEqual(Object.keys(result).sort(), [
  "hash",
  "hostname",
  "inputs",
  "password",
  "pathname",
  "port",
  "protocol",
  "search",
  "username",
]);
assert.strictEqual(result.hostname.input, "example.com");
assert.strictEqual(result.pathname.input, "/test");
assert.strictEqual(result.pathname.groups.value, "test");
console.log("URLPattern exec result passed");

const assert = require("assert");
const { exec } = require("child_process");

exec("ls").stdout.on("data", (chunk) => assert.strictEqual(typeof chunk, "string"));
exec("fhqwhgads").stderr.on("data", (chunk) => {
  assert.strictEqual(typeof chunk, "string");
});

const fixtures = require("../common/fixtures");
const assert = require("assert");

assert.strictEqual(typeof fixtures.fixturesDir, "string");
assert.strictEqual(
  fixtures.fixturesDir.endsWith("/tests/node/test/fixtures"),
  true,
);

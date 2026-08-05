const fixtures = require("../../node/test/common/fixtures");

if (typeof fixtures.readKey !== "function")
  throw new Error("local CommonJS exports were not loaded");
if (
  !fixtures.path("utf8_test_text.txt").includes("/fixtures/utf8_test_text.txt")
)
  throw new Error("local CommonJS __dirname resolution failed");

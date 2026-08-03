const fixtures = require("../common/fixtures");
if (
  typeof fixtures.fixturesDir !== "string" ||
  !fixtures.fixturesDir.endsWith("/tests/node/test/fixtures")
)
  throw new Error("fixturesDir missing");

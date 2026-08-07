const fixtures = require("../common/fixtures");

const value = fixtures.readSync("utf8_test_text.txt", "utf8");
if (typeof value !== "string" || value.length === 0) {
  throw new Error("common fixtures readSync did not read the fixture");
}

console.log("common fixtures readSync passed");

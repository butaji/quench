const assert = require("node:assert");

const fixture = require(
  "../../node/test/fixtures/wpt/url/resources/urltestdata.json",
);
assert(Array.isArray(fixture));
assert(fixture.length > 100);
console.log("Local JSON require passed");

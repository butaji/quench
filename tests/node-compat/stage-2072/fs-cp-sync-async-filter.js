const assert = require("assert");
const fs = require("fs");
const path = require("path");

const source = path.join(
  process.cwd(),
  "tests/node/test/fixtures/copy/kitchen-sink"
);
const destination = path.join(
  process.cwd(),
  "tests/node/test/.tmp.0/cp-async-filter"
);
assert.throws(
  () =>
    fs.cpSync(source, destination, {
      recursive: true,
      filter: async () => true
    }),
  { code: "ERR_INVALID_RETURN_VALUE" }
);

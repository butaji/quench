"use strict";
const assert = require("assert");
const fs = require("fs");
const fixtures = require("../../tests/node/test/common/fixtures");
const path = fs.realpathSync(fixtures.fixturesDir);
const expected = Buffer.from(path);
for (const encoding of ["ascii", "utf8", "utf16le", "ucs2", "base64", "binary", "hex"]) {
  const got = fs.realpathSync(path, { encoding });
  assert.strictEqual(got, expected.toString(encoding), encoding);
  const stringGot = fs.realpathSync(path, encoding);
  assert.strictEqual(stringGot, expected.toString(encoding), `string:${encoding}`);
  const bufferGot = fs.realpathSync(expected, { encoding });
  assert.strictEqual(bufferGot, expected.toString(encoding), `buffer:${encoding}`);
  const bufferStringGot = fs.realpathSync(expected, encoding);
  assert.strictEqual(bufferStringGot, expected.toString(encoding), `buffer-string:${encoding}`);
}
const expectedByKey = {};
for (const encoding of ["ascii", "utf8", "utf16le", "ucs2", "base64", "binary", "hex"]) expectedByKey[encoding] = expected.toString(encoding);
let encoding;
for (encoding in expectedByKey) {
  assert.strictEqual(fs.realpathSync(path, { encoding }), expectedByKey[encoding], `for-in:${encoding}`);
}

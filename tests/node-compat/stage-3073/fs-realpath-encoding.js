"use strict";

const assert = require("assert");
const fs = require("fs");
const fixtures = require("../../tests/node/test/common/fixtures");

const path = fs.realpathSync(fixtures.fixturesDir);
const expected = Buffer.from(path);
const asyncRealpath = (input, options) =>
  new Promise((resolve, reject) => {
    fs.realpath(input, options, (error, value) =>
      error ? reject(error) : resolve(value)
    );
  });

assert.strictEqual(
  fs.realpathSync(path, { encoding: "hex" }),
  expected.toString("hex")
);
assert.deepStrictEqual(fs.realpathSync(path, { encoding: "buffer" }), expected);

Promise.all([
  asyncRealpath(path, { encoding: "hex" }).then((value) =>
    assert.strictEqual(value, expected.toString("hex"))
  ),
  asyncRealpath(path, "hex").then((value) =>
    assert.strictEqual(value, expected.toString("hex"))
  ),
  asyncRealpath(path, { encoding: "buffer" }).then((value) =>
    assert.deepStrictEqual(value, expected)
  ),
  asyncRealpath(expected, { encoding: "hex" }).then((value) =>
    assert.strictEqual(value, expected.toString("hex"))
  ),
  asyncRealpath(expected, { encoding: "buffer" }).then((value) =>
    assert.deepStrictEqual(value, expected)
  )
]);

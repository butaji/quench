'use strict';
const common = require('../common');
const fixtures = require('../common/fixtures');
const assert = require('assert');
const fs = require('fs');
const string_dir = fs.realpathSync(fixtures.fixturesDir);
const buffer_dir = Buffer.from(string_dir);
const encodings = ['ascii', 'utf8', 'utf16le', 'ucs2', 'base64', 'binary', 'hex'];
const expected = {};
for (const encoding of encodings) expected[encoding] = buffer_dir.toString(encoding);
let encoding;
for (encoding in expected) {
  const expected_value = expected[encoding];
  let result;
  result = fs.realpathSync(string_dir, { encoding });
  assert.strictEqual(result, expected_value, `object-string:${encoding}`);
  result = fs.realpathSync(string_dir, encoding);
  assert.strictEqual(result, expected_value, `string-string:${encoding}`);
  result = fs.realpathSync(buffer_dir, { encoding });
  assert.strictEqual(result, expected_value, `object-buffer:${encoding}`);
  result = fs.realpathSync(buffer_dir, encoding);
  assert.strictEqual(result, expected_value, `string-buffer:${encoding}`);
}
let buffer_result;
buffer_result = fs.realpathSync(string_dir, { encoding: 'buffer' });
assert.deepStrictEqual(buffer_result, buffer_dir);
buffer_result = fs.realpathSync(string_dir, 'buffer');
assert.deepStrictEqual(buffer_result, buffer_dir);
buffer_result = fs.realpathSync(buffer_dir, { encoding: 'buffer' });
assert.deepStrictEqual(buffer_result, buffer_dir);
buffer_result = fs.realpathSync(buffer_dir, 'buffer');
assert.deepStrictEqual(buffer_result, buffer_dir);
for (encoding in expected) {
  const expected_value = expected[encoding];
  fs.realpath(string_dir, { encoding }, common.mustSucceed((res) => {
    assert.strictEqual(res, expected_value, `async-object-string:${encoding}`);
  }));
  fs.realpath(string_dir, encoding, common.mustSucceed((res) => {
    assert.strictEqual(res, expected_value, `async-string-string:${encoding}`);
  }));
  fs.realpath(buffer_dir, { encoding }, common.mustSucceed((res) => {
    assert.strictEqual(res, expected_value, `async-object-buffer:${encoding}`);
  }));
  fs.realpath(buffer_dir, encoding, common.mustSucceed((res) => {
    assert.strictEqual(res, expected_value, `async-string-buffer:${encoding}`);
  }));
}
fs.realpath(string_dir, { encoding: 'buffer' }, common.mustSucceed((res) => {
  assert.deepStrictEqual(res, buffer_dir);
}));
fs.realpath(string_dir, 'buffer', common.mustSucceed((res) => {
  assert.deepStrictEqual(res, buffer_dir);
}));
fs.realpath(buffer_dir, { encoding: 'buffer' }, common.mustSucceed((res) => {
  assert.deepStrictEqual(res, buffer_dir);
}));
fs.realpath(buffer_dir, 'buffer', common.mustSucceed((res) => {
  assert.deepStrictEqual(res, buffer_dir);
}));

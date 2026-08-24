const assert = require('assert');

const input = 'Cookie: abc=123\r\nCookie: def=456\r\nCookie: ghi=789';
assert.deepStrictEqual(
  input.match(/^Cookie: .+$/img),
  ['Cookie: abc=123', 'Cookie: def=456', 'Cookie: ghi=789'],
);

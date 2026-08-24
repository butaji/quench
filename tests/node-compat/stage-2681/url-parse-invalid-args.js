const assert = require('assert');
const url = require('url');

for (const value of [undefined, null, true, 0, [], {}, () => {}, Symbol('foo')]) {
  assert.throws(() => url.parse(value), {
    code: 'ERR_INVALID_ARG_TYPE',
    name: 'TypeError',
  });
}

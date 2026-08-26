const assert = require('assert');
const url = require('url');

let received = 0;
url.parse('foo');
process.on('warning', (warning) => {
  received += 1;
  assert.strictEqual(warning.name, 'DeprecationWarning');
  assert.strictEqual(warning.code, 'DEP0169');
});
process.nextTick(() => assert.strictEqual(received, 1));

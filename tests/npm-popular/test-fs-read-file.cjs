const assert = require('assert');
const fs = require('fs');

assert.throws(() => fs.readFile(__filename, { encoding: 'foo-8' }, () => {}), {
  code: 'ERR_INVALID_ARG_VALUE',
  name: 'TypeError',
});

fs.readFile(__filename, 'utf8', (error, source) => {
  assert.ifError(error);
  assert(source.includes('fs.readFile'));
});

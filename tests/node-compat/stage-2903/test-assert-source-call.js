const assert = require('assert');
const ässört = require('assert');

try {
  ässört.ok('');
} catch (error) {
  assert.match(error.message, /ässört\.ok\(''\)/);
}

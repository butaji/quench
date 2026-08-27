'use strict';

const assert = require('assert');

let warning;
process.on('warning', (value) => {
  warning = value;
});
const timer = setTimeout(() => {}, -1);
clearTimeout(timer);
setImmediate(() => {
  assert.strictEqual(warning.name, 'TimeoutNegativeWarning');
  assert.match(warning.message, /-1 is a negative number/);
});

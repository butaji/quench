'use strict';

const assert = require('assert');
const util = require('util');

let count = 0;
process.on('warning', (warning) => {
  if (warning.name === 'DeprecationWarning' && warning.message === 'compat') count++;
});
const wrapped = util.deprecate(() => {}, 'compat');
wrapped();
wrapped();
process.on('exit', () => assert.strictEqual(count, 1));

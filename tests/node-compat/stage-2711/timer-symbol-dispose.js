'use strict';

const assert = require('assert');

for (const timer of [
  setTimeout(() => {}, 1000),
  setInterval(() => {}, 1000),
  setImmediate(() => {}),
]) {
  assert.strictEqual(typeof timer[Symbol.dispose], 'function');
  timer[Symbol.dispose]();
  assert.strictEqual(timer._destroyed, true);
  timer[Symbol.dispose]();
}

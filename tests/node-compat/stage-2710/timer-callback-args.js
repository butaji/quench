'use strict';
const assert = require('assert');
let seen;
function callback(value) {
  seen = value;
}
setTimeout(callback, 1, 42);
setTimeout(() => {
  assert.strictEqual(seen, 42);
}, 5);

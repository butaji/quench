// Node compat: assert module shape.
const assert = require('node:assert');
if (typeof assert.ok !== 'function') throw new Error('ok: ' + typeof assert.ok);
console.log('assert: ok');

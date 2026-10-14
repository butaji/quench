'use strict';

// Node's builtin loader uses its own primordials even before the first require.
const original = Object.defineProperty;
let calls = 0;
let outcome;
Object.defineProperty = function () {
  calls++;
  throw new Error('guest Object.defineProperty called during builtin loading');
};
try {
  const fs = require('node:fs');
  outcome = {
    loaded: true,
    readFileSync: typeof fs.readFileSync,
    open: typeof fs.promises.open,
    repeatedIdentity: require('node:fs') === fs,
  };
} catch (error) {
  outcome = { loaded: false, name: error.name, code: error.code, message: error.message };
} finally {
  Object.defineProperty = original;
}
console.log(JSON.stringify({ calls, outcome }));

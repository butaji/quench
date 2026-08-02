const assert = require('assert');
const fs = require('fs');
const a = fs.mkdtempSync('/tmp/quench-node-same-');
const b = fs.mkdtempSync('/tmp/quench-node-same-');
assert.notStrictEqual(a, b);
fs.rmdirSync(a);
fs.rmdirSync(b);

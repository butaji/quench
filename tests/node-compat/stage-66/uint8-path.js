const assert = require('assert');
const fs = require('fs');
const path = '/tmp/\u0222abc.';
const folder = fs.mkdtempSync(new TextEncoder().encode(path));
assert.strictEqual(fs.existsSync(folder), true);
fs.rmdirSync(folder);

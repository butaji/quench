const assert = require('assert');
const { execFileSync } = require('child_process');
const args = ['-e', 'console.log("this is stdout");'];
assert.throws(() => execFileSync(process.execPath, args, { maxBuffer: 1 }));
const value = execFileSync(process.execPath, args, { maxBuffer: Infinity });
assert.ok(value);

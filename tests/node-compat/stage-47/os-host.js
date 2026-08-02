const assert = require('assert');
const os = require('os');
assert.strictEqual(os.tmpdir().length > 0, true);
assert.strictEqual(os.homedir().length > 0, true);
assert.strictEqual(os.hostname().length > 0, true);

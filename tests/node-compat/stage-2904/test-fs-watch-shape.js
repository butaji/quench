const assert = require('assert');
const fs = require('fs');

const directory = fs.mkdtempSync('/tmp/quench-watch-');
const watcher = fs.watch(directory, { recursive: true });
assert.strictEqual(typeof watcher.on, 'function');
assert.strictEqual(typeof watcher.close, 'function');
assert.strictEqual(watcher.close(), undefined);

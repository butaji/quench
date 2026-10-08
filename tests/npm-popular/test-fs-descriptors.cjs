const assert = require('assert');
const fs = require('fs');

const fd = fs.openSync(__filename, 'r');
assert.strictEqual(typeof fd, 'number');
assert.strictEqual(fs.closeSync(fd), undefined);

const numericFd = fs.openSync(__filename, fs.constants.O_RDONLY);
assert.strictEqual(fs.closeSync(numericFd), undefined);

assert.throws(() => fs.closeSync(fd), { code: 'EBADF' });

fs.open(__filename, (error, openedFd) => {
  assert.ifError(error);
  fs.close(openedFd, (closeError) => assert.ifError(closeError));
});

fs.promises.open(__filename).then((handle) => handle.close());

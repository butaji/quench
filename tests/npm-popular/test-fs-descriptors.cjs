const assert = require('assert');
const fs = require('fs');

const fd = fs.openSync(__filename, 'r');
assert.strictEqual(typeof fd, 'number');
assert.strictEqual(fs.closeSync(fd), undefined);

const numericFd = fs.openSync(__filename, fs.constants.O_RDONLY);
assert.strictEqual(fs.closeSync(numericFd), undefined);

assert.throws(() => fs.closeSync(fd), { code: 'EBADF' });

const filename = `${__dirname}/.fs-descriptor-${process.pid}`;
try {
  const dataFd = fs.openSync(filename, 'w+');
  assert.strictEqual(fs.writeSync(dataFd, 'foo'), 3);
  assert.strictEqual(fs.writeSync(dataFd, Buffer.from('bar'), 0, 3), 3);
  assert.strictEqual(fs.fstatSync(dataFd).size, 6);
  const output = Buffer.alloc(6);
  assert.strictEqual(fs.readSync(dataFd, output, 0, output.length, 0), 6);
  assert.strictEqual(output.toString(), 'foobar');
  fs.closeSync(dataFd);
  assert.strictEqual(fs.readFileSync(filename, 'utf8'), 'foobar');
} finally {
  fs.rmSync(filename, { force: true });
}

fs.open(__filename, (error, openedFd) => {
  assert.ifError(error);
  fs.fstat(openedFd, (statError, stats) => {
    assert.ifError(statError);
    assert.strictEqual(stats.isFile(), true);
    fs.close(openedFd, (closeError) => assert.ifError(closeError));
  });
});

fs.promises.open(__filename).then(async (handle) => {
  const output = Buffer.alloc(5);
  const result = await handle.read(output, 0, output.length, 0);
  assert.strictEqual(result.bytesRead, output.length);
  assert.strictEqual((await handle.stat()).isFile(), true);
  await handle.close();
});

fs.open(__filename, 'r', (error, readFd) => {
  assert.ifError(error);
  const output = Buffer.alloc(5);
  fs.read(readFd, output, 0, output.length, 0, (readError, bytesRead) => {
    assert.ifError(readError);
    assert.strictEqual(bytesRead, output.length);
    fs.closeSync(readFd);
  });
});

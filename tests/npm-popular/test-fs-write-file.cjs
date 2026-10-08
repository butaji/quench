const assert = require('assert');
const fs = require('fs');

const path = `${__dirname}/.fs-write-file-${process.pid}`;
fs.writeFile(path, 'callback', (error) => {
  assert.ifError(error);
  assert.strictEqual(fs.readFileSync(path, 'utf8'), 'callback');
  fs.promises.writeFile(path, 'promise').then(() => {
    assert.strictEqual(fs.readFileSync(path, 'utf8'), 'promise');
    fs.rmSync(path, { force: true });
  });
});

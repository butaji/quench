const assert = require('assert');
const fs = require('fs');

const path = `${__dirname}/.fs-append-file-${process.pid}`;
fs.appendFile(path, 'a', (error) => {
  assert.ifError(error);
  fs.promises.appendFile(path, 'b').then(() => {
    assert.strictEqual(fs.readFileSync(path, 'utf8'), 'ab');
    fs.rmSync(path, { force: true });
  });
});

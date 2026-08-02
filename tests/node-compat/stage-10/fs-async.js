const assert = require('assert');
const fs = require('fs');
const folder = fs.mkdtempSync('/tmp/quench-node-');
const file = `${folder}/async.txt`;
fs.writeFile(file, 'async data', (error) => {
  assert.ifError(error);
  fs.readFile(file, 'utf8', (readError, data) => {
    assert.ifError(readError);
    assert.strictEqual(data, 'async data');
  });
});
fs.promises.writeFile(file, 'promise data').then(() => fs.promises.readFile(file, 'utf8')).then((data) => {
  assert.strictEqual(data, 'promise data');
});

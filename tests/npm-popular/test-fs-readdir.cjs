const assert = require('assert');
const fs = require('fs');

for (const path of [false, 1, [], {}, null, undefined]) {
  assert.throws(() => fs.readdirSync(path), {
    code: 'ERR_INVALID_ARG_TYPE',
    name: 'TypeError',
  });
  assert.throws(() => fs.readdir(path, () => {}), {
    code: 'ERR_INVALID_ARG_TYPE',
    name: 'TypeError',
  });
}

const bufferPath = Buffer.from(__dirname);
assert.deepStrictEqual(fs.readdirSync(bufferPath), fs.readdirSync(__dirname));

fs.readdir(__dirname, (error, entries) => {
  assert.ifError(error);
  assert(entries.includes('test-fs-readdir.cjs'));
});

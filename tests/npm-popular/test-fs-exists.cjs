'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const manifest = path.resolve('package.json');
assert.equal(fs.existsSync(manifest), true);
assert.equal(fs.existsSync(`${manifest}.missing`), false);
assert.throws(() => fs.exists(manifest), { code: 'ERR_INVALID_ARG_TYPE' });
assert.equal(fs.accessSync(manifest), undefined);
assert.throws(() => fs.accessSync(`${manifest}.missing`), { code: 'ENOENT' });
assert.throws(() => fs.accessSync(manifest, {}), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => fs.accessSync(manifest, -1), { code: 'ERR_OUT_OF_RANGE' });
assert.throws(() => fs.accessSync(100), { code: 'ERR_INVALID_ARG_TYPE' });
assert.throws(() => fs.access(100, () => {}), { code: 'ERR_INVALID_ARG_TYPE' });
const noExecute = path.join(process.cwd(), `quench-access-${process.pid}`);
fs.writeFileSync(noExecute, 'x', { mode: 0o600 });
assert.throws(() => fs.accessSync(noExecute, fs.constants.X_OK), { code: 'EACCES' });
fs.rmSync(noExecute);
const chmodTarget = path.join(process.cwd(), `quench-chmod-${process.pid}`);
const chmodPromiseTarget = path.join(process.cwd(), `quench-chmod-promise-${process.pid}`);
fs.writeFileSync(chmodTarget, 'x');
fs.writeFileSync(chmodPromiseTarget, 'x');
fs.chmodSync(chmodTarget, 0o600);
assert.equal(fs.statSync(chmodTarget).mode & 0o777, 0o600);
const chmodFd = fs.openSync(chmodTarget, 'r+');
fs.fchmodSync(chmodFd, 0o600);

let callbacks = 0;
fs.exists(manifest, (exists) => {
  assert.equal(exists, true);
  callbacks++;
});
fs.exists(`${manifest}.missing`, (exists) => {
  assert.equal(exists, false);
  callbacks++;
});
fs.access(manifest, (error) => {
  assert.ifError(error);
  callbacks++;
});
fs.promises.access(manifest).then(() => {
  callbacks++;
});
fs.chmod(chmodTarget, '640', (error) => {
  assert.ifError(error);
  assert.equal(fs.statSync(chmodTarget).mode & 0o777, 0o640);
  callbacks++;
  fs.fchmod(chmodFd, '620', (fdError) => {
    assert.ifError(fdError);
    assert.equal(fs.fstatSync(chmodFd).mode & 0o777, 0o620);
    fs.closeSync(chmodFd);
    callbacks++;
  });
});
fs.promises.chmod(chmodPromiseTarget, 0o604).then(() => {
  assert.equal(fs.statSync(chmodPromiseTarget).mode & 0o777, 0o604);
  callbacks++;
  fs.rmSync(chmodTarget);
  fs.rmSync(chmodPromiseTarget);
});
setTimeout(() => assert.equal(callbacks, 7), 25);

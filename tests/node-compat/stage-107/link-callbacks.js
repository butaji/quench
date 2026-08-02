const fs = require('fs');
const common = require('../common');

const target = '/tmp/quench-node-stage-107-target';
const link = '/tmp/quench-node-stage-107-link';
try { fs.unlinkSync(link); } catch (_) {}
fs.writeFileSync(target, 'link');
fs.symlink(target, link, common.mustSucceed(() => {
  fs.readlink(link, common.mustSucceed((value) => {
    if (value !== target) throw new Error('readlink mismatch');
  }));
}));

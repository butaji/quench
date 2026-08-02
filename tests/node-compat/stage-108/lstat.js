const fs = require('fs');

const target = `/tmp/quench-node-stage-108-target-${process.pid}`;
const link = `/tmp/quench-node-stage-108-link-${process.pid}`;
fs.writeFileSync(target, 'x');
fs.symlinkSync(target, link);
if (fs.statSync(link).isSymbolicLink()) throw new Error('stat followed symlink incorrectly');
if (!fs.lstatSync(link).isSymbolicLink()) throw new Error('lstat did not identify symlink');

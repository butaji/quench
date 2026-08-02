const fs = require('fs');

const path = `/tmp/quench-node-stage-110-${process.pid}`;
const fd = fs.openSync(path, 'w');
fs.writeFileSync(path, 'durable');
fs.fsyncSync(fd);
fs.fdatasyncSync(fd);
fs.fsync(fd, (error) => {
  if (error) throw error;
  fs.fdatasync(fd, (error2) => {
    if (error2) throw error2;
    fs.closeSync(fd);
  });
});

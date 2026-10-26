const cluster = require('node:cluster');
if (cluster.isPrimary) {
  cluster.on('listening', (worker) => {
    console.log('listening', worker.id);
    worker.kill();
  });
  cluster.fork();
} else {
  require('node:http').Server(() => {}).listen(0, '127.0.0.1');
}

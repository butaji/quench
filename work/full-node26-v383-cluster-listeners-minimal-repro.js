const cluster = require('cluster');
const net = require('net');
const workers = Number(process.env.WORKERS || 8);
if (cluster.isWorker) {
  for (let i = 0; i < 2; i++) net.createServer(s => s.end('echo')).listen(0, '127.0.0.1');
} else {
  const ports = [];
  for (let i = 0; i < workers; i++) {
    cluster.fork().on('listening', addr => {
      ports.push(addr.port);
      if (ports.length === workers * 2) {
        console.log('listening ports', JSON.stringify(ports));
        let i = 0;
        const next = () => {
          if (i === ports.length) return cluster.disconnect(() => console.log('disconnect done'));
          const port = ports[i++];
          const s = net.connect(port, '127.0.0.1'); let data='';
          s.on('data', d=>data+=d); s.on('error', e=>console.log('connection error', port, e.code));
          s.on('end',()=>{console.log('connected',port,data); next();});
        };
        next();
      }
    });
  }
}

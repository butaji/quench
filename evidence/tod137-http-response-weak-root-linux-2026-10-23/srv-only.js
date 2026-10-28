const { createServer } = require('node:http');
const server = createServer((req, res) => { res.writeHead(200, { 'Content-Length': 2 }); res.end('Hi'); });
server.listen(Number(process.argv[2]), '127.0.0.1', () => console.log('listening'));
process.on('SIGTERM', () => server.close());
setTimeout(() => server.close(), Number(process.argv[3]) * 1000);

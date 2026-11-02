const { Server } = require('socket.io'); const { io } = require('socket.io-client'); const http = require('node:http'); const fs = require('node:fs'); const path = require('node:path');
const routes = { '/': (q, s) => { s.setHeader('content-type', 'text/html'); s.end('<p>hi</p>'); },
  '/echo': (q, s) => { let b = ''; q.on('data', d => b += d); q.on('end', () => { s.setHeader('content-type', 'application/json'); s.end(b); }); },
  '/stream': (q, s) => { s.setHeader('content-type', 'text/plain'); s.write('a'); s.write('b'); s.end('c'); },
  '/asset.txt': (q, s) => { s.setHeader('content-type', 'text/plain'); fs.createReadStream(path.join(__dirname, 'asset.txt')).pipe(s); } };
// One client over HTTP long-polling only, one that upgrades to a websocket; each sends an
// acknowledged event and receives a broadcast.
const client = (port, transports) => new Promise((r, j) => { const c = io(`http://127.0.0.1:${port}`, { transports, upgrade: transports.length > 1, forceNew: true });
  c.on('connect_error', j);
  c.on('connect', () => c.emit('ping', { n: 1, s: 'x'.repeat(5000) }, (ack) => { c.once('news', (news) => {
    const transport = c.io.engine.transport.name; c.close(); r(`${transport} ack ${ack.n} ${ack.len} news ${news}`); }); c.emit('want-news'); })); });
exports.start = () => new Promise(r => { const srv = http.createServer((q, s) => (routes[q.url] || ((q, s) => { s.statusCode = 404; s.end('Not Found'); }))(q, s));
  const ioServer = new Server(srv);
  ioServer.on('connection', (sock) => { sock.on('ping', (m, ack) => ack({ n: m.n + 1, len: m.s.length })); sock.on('want-news', () => ioServer.to(sock.id).emit('news', 'fresh')); });
  srv.listen(0, async () => { const port = srv.address().port;
    console.log('polling', await client(port, ['polling']));
    console.log('websocket', await new Promise((done, j) => { const c = io(`http://127.0.0.1:${port}`, { forceNew: true });
      c.on('connect_error', j); c.io.engine.on('upgrade', (t) => c.emit('ping', { n: 5, s: 'y' }, (ack) => { c.close(); done(`${t.name} ack ${ack.n} ${ack.len}`); })); }));
    r({ port, stop: () => new Promise(d => { ioServer.close(); srv.close(d); }) }); }); });

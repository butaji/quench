const { WebSocketServer, WebSocket } = require('ws'); const http = require('node:http'); const fs = require('node:fs'); const path = require('node:path');
const routes = { '/': (q, s) => { s.setHeader('content-type', 'text/html'); s.end('<p>hi</p>'); },
  '/echo': (q, s) => { let b = ''; q.on('data', d => b += d); q.on('end', () => { s.setHeader('content-type', 'application/json'); s.end(b); }); },
  '/stream': (q, s) => { s.setHeader('content-type', 'text/plain'); s.write('a'); s.write('b'); s.end('c'); },
  '/asset.txt': (q, s) => { s.setHeader('content-type', 'text/plain'); fs.createReadStream(path.join(__dirname, 'asset.txt')).pipe(s); } };
// A text, a binary and a large message round trip, then a close with a code and reason.
const roundTrip = (port) => new Promise((r, j) => { const c = new WebSocket(`ws://127.0.0.1:${port}/socket`, { perMessageDeflate: true }); const got = [];
  c.on('open', () => { c.send('hello'); c.send(Buffer.from([1, 2, 3])); c.send('x'.repeat(70000)); });
  c.on('message', (m, binary) => { got.push(`${binary ? 'bin' : 'text'}:${m.length}`); if (got.length === 3) c.close(4000, 'done'); });
  c.on('close', (code, reason) => r(`${got.join(' ')} close ${code} ${reason}`)); c.on('error', j); });
exports.start = () => new Promise(r => { const srv = http.createServer((q, s) => (routes[q.url] || ((q, s) => { s.statusCode = 404; s.end('Not Found'); }))(q, s));
  const wss = new WebSocketServer({ server: srv, path: '/socket', perMessageDeflate: true });
  wss.on('connection', (sock, req) => { console.log('ws upgrade', req.url, sock.protocol === '', sock.extensions); sock.on('message', (m, binary) => sock.send(m, { binary })); });
  srv.listen(0, async () => { const port = srv.address().port; console.log('ws', await roundTrip(port));
    r({ port, stop: () => new Promise(d => { wss.close(); srv.close(d); }) }); }); });

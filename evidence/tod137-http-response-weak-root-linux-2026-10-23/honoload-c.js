// Keep-alive loopback load on a Hono-style route table: N requests over C concurrent sockets.
const { createServer } = require('node:http');
const net = require('node:net');
const N = Number(process.argv[2] || 2000), C = Number(process.argv[3] || 64);
const routes = [];
const app = { get(p, h) { routes.push({ p, h }); },
  dispatch(req) {
    const u = new URL(req.url, 'http://localhost');
    for (const r of routes) { const m = r.p.exec(u.pathname); if (m) return r.h(m, u); }
    return { status: 404, body: 'Not Found' };
  } };
app.get(/^\/$/, () => ({ status: 200, body: 'Hi' }));
app.get(/^\/id\/([^/]+)$/, (m, u) => ({ status: 200, body: `${m[1]} ${u.searchParams.get('name')}`, headers: { 'x-powered-by': 'benchmark' } }));
const server = createServer((req, res) => {
  const { status, body, headers } = app.dispatch(req);
  res.writeHead(status, { 'Content-Type': 'text/plain', 'Content-Length': Buffer.byteLength(body), ...headers });
  res.end(body);
});
let sent = 0, done = 0;
server.listen(0, () => {
  const port = server.address().port;
  const clients = [];
  for (let i = 0; i < C; i++) {
    const c = net.connect(port, '127.0.0.1');
    clients.push(c);
    let buf = '';
    const send = () => { if (sent < N) c.write(`GET /id/${sent++}?name=bun HTTP/1.1\r\nHost: localhost\r\n\r\n`); };
    c.setEncoding('latin1');
    c.on('connect', send);
    c.on('data', (d) => {
      buf += d;
      for (;;) {
        const end = buf.indexOf('\r\n\r\n');
        if (end < 0) return;
        const len = Number(/content-length: (\d+)/i.exec(buf.slice(0, end))[1]);
        if (buf.length < end + 4 + len) return;
        buf = buf.slice(end + 4 + len);
        if (++done === N) { console.log('done', done); for (const k of clients) k.end(); server.close(); return; }
        send();
      }
    });
  }
});

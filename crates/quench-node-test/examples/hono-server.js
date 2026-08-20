// Smallest possible Hono-style server built only on the `node:` surface
// `quench-node` exposes. A route table (`app.get`) dispatches each
// request through a fetch-style handler; `http.createServer` serves it
// over a real loopback socket, and a `net` client asserts the response.

const { createServer } = require('node:http');
const net = require('node:net');

function createHono() {
  const routes = [];
  const app = {
    get(path, handler) {
      routes.push({ method: 'GET', path, handler });
    },
    // Fetch-style dispatch: returns (status, body) for a request.
    dispatch(req) {
      for (const r of routes) {
        if (r.method === req.method && r.path === req.url) {
          return r.handler({ req });
        }
      }
      return { status: 404, body: 'Not Found' };
    },
  };
  return app;
}

const app = createHono();
app.get('/', () => ({ status: 200, body: 'hello hono' }));
app.get('/json', (c) => ({ status: 200, body: '{"ok":true}' }));

const server = createServer((req, res) => {
  const { status, body } = app.dispatch(req);
  res.writeHead(status, { 'Content-Type': 'text/plain' });
  res.end(body);
});

server.listen(0, () => {
  const port = server.address().port;

  const client = net.connect(port, '127.0.0.1', () => {
    client.write('GET /json HTTP/1.1\r\nHost: localhost\r\n\r\n');
  });
  let received = '';
  client.setEncoding('utf8');
  client.on('data', (chunk) => { received += chunk; });
  client.on('end', () => {
    if (!/^HTTP\/1\.1 200 OK/.test(received)) throw new Error('bad status');
    if (!received.endsWith('{"ok":true}')) throw new Error('bad body');
    if (!/Content-Type: text\/plain/.test(received)) throw new Error('bad content-type');
    server.close(() => console.log('hono-server: ok (routed over real http)'));
  });
  client.on('error', () => {});
});
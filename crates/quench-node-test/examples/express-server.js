// Smallest possible Express-style app using only the Node API surface
// that `quench-node` exposes. `node:http`'s createServer is real, so
// the app builds a route table and serves actual requests over a
// loopback socket: a `net` client issues one HTTP request and asserts
// the response the route table produced.

const http = require('node:http');
const { createServer } = http;
const net = require('node:net');
const assert = require('assert');

function createApp() {
  const middlewares = [];
  const routes = [];
  const app = {
    use(fn) {
      middlewares.push(fn);
    },
    get(path, handler) {
      routes.push({ method: 'GET', path, handler });
    },
    post(path, handler) {
      routes.push({ method: 'POST', path, handler });
    },
    listen(port, cb) {
      const server = createServer((req, res) => app.handle(req, res));
      return server.listen(port, cb);
    },
    handle(req, res) {
      for (const mw of middlewares) mw(req, res);
      for (const r of routes) {
        if (r.method === req.method && r.path === req.url) {
          return r.handler(req, res);
        }
      }
      res.statusCode = 404;
      res.end('not found');
    },
  };
  return app;
}

const app = createApp();
app.use((req, res) => res.setHeader('x-handled-by', 'quench'));
app.get('/hello', (req, res) => res.end('{"msg":"hi"}'));
app.get('/users/1', (req, res) => res.end('{"id":1}'));
app.post('/echo', (req, res) => {
  let body = '';
  req.on('data', (chunk) => { body += chunk.toString(); });
  req.on('end', () => res.end('echo:' + body));
});

const server = app.listen(0, () => {
  const port = server.address().port;

  // GET round-trip.
  const client = net.connect(port, '127.0.0.1', () => {
    client.write('GET /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n');
  });
  let received = '';
  client.setEncoding('utf8');
  client.on('data', (chunk) => { received += chunk; });
  client.on('end', () => {
    assert.match(received, /^HTTP\/1\.1 200 OK/, 'status line');
    assert.ok(received.endsWith('{"msg":"hi"}'), 'routed body');
    assert.match(received, /x-handled-by: quench/i, 'middleware header');

    // POST round-trip with a request body through the route table.
    const post = http.request(
      { host: '127.0.0.1', port, method: 'POST', path: '/echo' },
      (res) => {
        let out = '';
        res.on('data', (chunk) => { out += chunk.toString(); });
        res.on('end', () => {
          assert.strictEqual(out, 'echo:payload', 'POST body echoed');
          server.close(() => {
            console.log('express-server: ok (GET + POST over real http)');
          });
        });
      }
    );
    post.end('payload');
  });
  client.on('error', () => {});
});
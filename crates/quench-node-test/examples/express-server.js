// Smallest possible Express-style app using only the Node API
// surface that `quench-node` exposes. `node:http`'s createServer
// is currently a shape stub in the host: it returns an object
// whose `listen`/`close` are `undefined` and it never stores the
// request handler or serves a socket. So this example detects
// that and simulates the request/response cycle in-process,
// dispatching fake req/res objects through the route table.

const { createServer } = require('node:http');

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
    listen(port, cb) {
      const server = createServer((req, res) => app.handle(req, res));
      if (server && typeof server.listen === 'function') {
        return server.listen(port, cb); // real host someday
      }
      // Stub host: no sockets. Run cb so callers see "listening".
      if (cb) cb();
      return server;
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
app.use((req) => console.log('mw: %s %s', req.method, req.url));
app.get('/hello', (req, res) => res.end('{"msg":"hi"}'));
app.get('/users/1', (req, res) => res.end('{"id":1}'));

const results = [];
function fakeRes(req) {
  return {
    statusCode: 200,
    end(body) {
      results.push(req.method + ' ' + req.url + ' -> ' + this.statusCode + ' ' + body);
    },
  };
}

const server = app.listen(3000, () => console.log('listening on 3000 (simulated)'));
for (const req of [
  { method: 'GET', url: '/hello' },
  { method: 'GET', url: '/users/1' },
  { method: 'GET', url: '/missing' },
]) {
  app.handle(req, fakeRes(req));
}

const expected = [
  'GET /hello -> 200 {"msg":"hi"}',
  'GET /users/1 -> 200 {"id":1}',
  'GET /missing -> 404 not found',
];
let ok = 0;
for (let i = 0; i < expected.length; i++) {
  if (results[i] === expected[i]) {
    ok += 1;
    console.log('OK %s', results[i]);
  } else {
    console.log('MISMATCH want=%s got=%s', expected[i], results[i]);
  }
}
console.log('express-server: server shape %s', typeof server);
if (ok !== expected.length) process.exit(1);
console.log('express-server: ok');

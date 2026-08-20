// Smallest possible Next.js-style server built only on the `node:` surface
// `quench-node` exposes. Next.js dev/build pipelines are out of v1 scope
// (ADR 0002), so this example demonstrates the Next.js handler/routing shape
// — a `NextServer` with `prepare()` + per-request `handler(req)` returning
// a fetch-style Response — built on `node:http` and verified over a real
// loopback socket by a `node:net` client.
//
// The shape mirrors Next.js's request lifecycle: a long-lived server
// object is prepared once, then each request resolves a route from the
// pages map and runs the handler. Static assets are served via a
// catch-all `/_next/static` handler.

const { createServer } = require('node:http');
const net = require('node:net');

function createNextServer() {
  const pages = {};
  const server = {
    // Register a page handler at a route.
    page(path, handler) {
      pages[path] = handler;
    },
    // Prepare the server (placeholder for build/manifest work).
    prepare() {
      server._prepared = true;
      return server;
    },
    // Fetch-style dispatch for one request.
    async handler(req) {
      if (!server._prepared) throw new Error('server not prepared');
      const full = req.url || '/';
      const path = full.split('?')[0];
      const page = pages[path];
      if (page) return page({ req });
      // Next.js serves static assets from /_next/static.
      if (path.startsWith('/_next/static/')) {
        return { status: 200, body: '/* static asset */' };
      }
      return { status: 404, body: '404 Not Found' };
    },
  };
  return server;
}

const app = createNextServer();
app.page('/', () => ({ status: 200, body: 'hello next' }));
app.page('/api/hello', () => ({ status: 200, body: '{"framework":"next"}' }));
app.page('/api/echo', (c) => ({ status: 200, body: '{"path":"' + c.req.url + '"}' }));

const server = createServer(async (req, res) => {
  const out = await app.handler(req);
  res.writeHead(out.status, { 'Content-Type': 'text/plain' });
  res.end(out.body);
});

server.listen(0, () => {
  const port = server.address().port;
  const tests = [
    { path: '/api/hello', expect: '{"framework":"next"}' },
    { path: '/api/echo?x=1', expect: '{"path":"/api/echo?x=1"}' },
  ];
  let i = 0;
  function next() {
    if (i >= tests.length) {
      server.close(() => console.log('nextjs-server: ok (routed over real http)'));
      return;
    }
    const t = tests[i++];
    const client = net.connect(port, '127.0.0.1', () => {
      client.write('GET ' + t.path + ' HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n');
    });
    let received = '';
    client.setEncoding('utf8');
    client.on('data', (chunk) => { received += chunk; });
    client.on('end', () => {
      if (!/^HTTP\/1\.1 200 OK/.test(received)) throw new Error('bad status for ' + t.path);
      if (!received.endsWith(t.expect)) throw new Error('bad body for ' + t.path + ': ' + received);
      next();
    });
    client.on('error', () => {});
  }
  app.prepare();
  next();
});

const assert = require('assert');
const http = require('http');

assert.strictEqual(typeof http.Server, 'function');
const server = new http.Server((req, res) => {
  assert.strictEqual(req.method, 'GET');
  assert.strictEqual(req.url, '/stage-2669');
  req.on('end', () => res.end('ok'));
  req.resume();
});

server.on('listening', () => {
  const port = server.address().port;
  const agent = new http.Agent({ port, maxSockets: 1 });
  http.get({ port, path: '/stage-2669', agent }, (res) => {
    assert.strictEqual(res.statusCode, 200);
    res.on('data', () => {});
    res.on('end', () => server.close());
  });
});
server.listen(0);

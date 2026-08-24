const assert = require('assert');
const http = require('http');

const server = http.createServer((req, res) => {
  assert.strictEqual(req.headers.cookie, 'a=1; b=2; c=3');
  res.end('ok');
});
server.on('listening', () => {
  const req = http.request({
    port: server.address().port,
    headers: [['Cookie', 'a=1'], ['Cookie', 'b=2'], ['Cookie', 'c=3']],
  }, (res) => {
    res.on('data', () => {});
    res.on('end', () => server.close());
  });
  req.end();
});
server.listen(0);

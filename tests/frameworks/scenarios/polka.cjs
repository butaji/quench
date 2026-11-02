const polka = require('polka'); const { Readable } = require('node:stream'); const fs = require('node:fs'); const path = require('node:path');
const json = (q, res, next) => { let b = ''; q.on('data', d => b += d); q.on('end', () => { q.body = b ? JSON.parse(b) : {}; next(); }); };
exports.start = () => new Promise(r => { const a = polka({ onNoMatch: (q, res) => { res.statusCode = 404; res.setHeader('content-type', 'text/plain'); res.end('Not Found'); } });
  a.get('/', (q, res) => { res.setHeader('content-type', 'text/html'); res.end('<p>hi</p>'); });
  a.post('/echo', json, (q, res) => { res.setHeader('content-type', 'application/json'); res.end(JSON.stringify(q.body)); });
  a.get('/stream', (q, res) => { res.setHeader('content-type', 'text/plain'); Readable.from(['a', 'b', 'c']).pipe(res); });
  a.get('/asset.txt', (q, res) => { res.setHeader('content-type', 'text/plain'); fs.createReadStream(path.join(__dirname, 'asset.txt')).pipe(res); });
  a.listen(0, () => r({ port: a.server.address().port, stop: () => new Promise(d => a.server.close(d)) })); });

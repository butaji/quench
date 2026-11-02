const restify = require('restify'); const { Readable } = require('node:stream');
exports.start = () => new Promise(r => { const s = restify.createServer(); s.use(restify.plugins.bodyParser());
  s.get('/', (q, res, next) => { res.setHeader('content-type', 'text/html'); res.sendRaw(200, '<p>hi</p>'); next(); });
  s.post('/echo', (q, res, next) => { res.json(q.body); next(); });
  s.get('/stream', (q, res, next) => { res.setHeader('content-type', 'text/plain'); Readable.from(['a', 'b', 'c']).pipe(res); res.on('finish', () => next()); });
  s.get('/asset.txt', restify.plugins.serveStatic({ directory: __dirname }));
  s.listen(0, '127.0.0.1', () => r({ port: s.address().port, stop: () => new Promise(d => s.close(d)) })); });

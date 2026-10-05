const fastify = require('fastify'); const fs = require('node:fs'); const path = require('node:path'); const { Readable } = require('node:stream');
exports.start = async () => { const a = fastify();
  a.get('/', async (q, s) => s.type('text/html').send('<p>hi</p>')); a.post('/echo', async q => q.body);
  a.get('/stream', (q, s) => s.type('text/plain').send(Readable.from(['a', 'b', 'c'])));
  a.get('/asset.txt', (q, s) => s.type('text/plain').send(fs.createReadStream(path.join(__dirname, 'asset.txt'))));
  await a.listen({ port: 0 }); return { port: a.server.address().port, stop: () => a.close() }; };

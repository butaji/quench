const Hapi = require('@hapi/hapi'); const { Readable } = require('node:stream'); const fs = require('node:fs'); const path = require('node:path');
exports.start = async () => { const s = Hapi.server({ port: 0, host: '127.0.0.1' });
  s.route({ method: 'GET', path: '/', handler: (q, h) => h.response('<p>hi</p>').type('text/html') });
  s.route({ method: 'POST', path: '/echo', handler: (q) => q.payload });
  s.route({ method: 'GET', path: '/stream', handler: (q, h) => h.response(Readable.from(['a', 'b', 'c'], { objectMode: false })).type('text/plain') });
  s.route({ method: 'GET', path: '/asset.txt', handler: (q, h) => h.response(fs.createReadStream(path.join(__dirname, 'asset.txt'))).type('text/plain') });
  await s.start(); return { port: s.info.port, stop: () => s.stop() }; };

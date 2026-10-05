const express = require('express'); const { Readable } = require('node:stream');
exports.start = () => new Promise(r => { const a = express(); a.use(express.json()); a.use(express.static(__dirname, { index: false }));
  a.get('/', (q, s) => s.send('<p>hi</p>')); a.post('/echo', (q, s) => s.json(q.body));
  a.get('/stream', (q, s) => { s.type('text'); Readable.from(['a', 'b', 'c']).pipe(s); });
  const srv = a.listen(0, () => r({ port: srv.address().port, stop: () => new Promise(d => srv.close(d)) })); });

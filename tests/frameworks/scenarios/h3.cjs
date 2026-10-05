const fs = require('node:fs'); const path = require('node:path'); const http = require('node:http');
exports.start = async () => { const h3 = await import('h3'); const app = new h3.H3();
  app.get('/', () => h3.html`<p>hi</p>`); app.post('/echo', e => e.req.json());
  app.get('/stream', () => new Response(new ReadableStream({ start(k) { for (const x of 'abc') k.enqueue(new TextEncoder().encode(x)); k.close(); } })));
  app.get('/asset.txt', () => new Response(fs.readFileSync(path.join(__dirname, 'asset.txt')), { headers: { 'content-type': 'text/plain' } }));
  const { toNodeHandler } = await import('h3/node');
  return new Promise(r => { const srv = http.createServer(toNodeHandler(app)); srv.listen(0, () => r({ port: srv.address().port, stop: () => new Promise(d => srv.close(d)) })); }); };

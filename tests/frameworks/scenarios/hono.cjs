exports.start = async () => { const { Hono } = await import('hono'); const { serve } = await import('@hono/node-server'); const { serveStatic } = await import('@hono/node-server/serve-static');
  const a = new Hono();
  a.get('/', c => c.html('<p>hi</p>')); a.post('/echo', async c => c.json(await c.req.json()));
  a.get('/stream', c => c.body(new ReadableStream({ start(k) { for (const x of 'abc') k.enqueue(new TextEncoder().encode(x)); k.close(); } })));
  a.use('/asset.txt', serveStatic({ root: require('path').relative(process.cwd(), __dirname) || '.' }));
  return new Promise(r => { const srv = serve({ fetch: a.fetch, port: 0 }, info => r({ port: info.port, stop: () => new Promise(d => srv.close(d)) })); }); };

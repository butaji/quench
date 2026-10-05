const Koa = require('koa'); const fs = require('node:fs'); const path = require('node:path'); const { Readable } = require('node:stream');
exports.start = () => new Promise(r => { const a = new Koa();
  a.use(async ctx => {
    if (ctx.path === '/') ctx.body = '<p>hi</p>';
    else if (ctx.path === '/echo') { let s = ''; for await (const c of ctx.req) s += c; ctx.body = JSON.parse(s); }
    else if (ctx.path === '/stream') ctx.body = Readable.from(['a', 'b', 'c']);
    else if (ctx.path === '/asset.txt') { ctx.type = 'text'; ctx.body = fs.createReadStream(path.join(__dirname, 'asset.txt')); }
  });
  const srv = a.listen(0, () => r({ port: srv.address().port, stop: () => new Promise(d => srv.close(d)) })); });

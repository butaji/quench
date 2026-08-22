const Koa = require('koa');
const app = new Koa();
app.use(ctx => { if (ctx.path === '/health') ctx.body = { ok: true, framework: 'koa' }; });
app.listen(3458, '127.0.0.1');

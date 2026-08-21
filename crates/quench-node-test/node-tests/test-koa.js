const assert = require('node:assert');
const Koa = require('node:koa');
const app = new Koa();
app.use(async (ctx, next) => { ctx.state.seen = true; await next(); });
app.use(async ctx => { ctx.status = 201; ctx.body = { ok: ctx.state.seen }; });
assert.strictEqual(typeof app.callback(), 'function');
assert.strictEqual(app.middleware.length, 2);
console.log('koa compatibility ok');

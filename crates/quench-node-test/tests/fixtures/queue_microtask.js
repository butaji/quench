const ah = require('async_hooks');
const out = [];
const ids = () => ah.executionAsyncId() + '/' + ah.triggerAsyncId();
queueMicrotask(function () { out.push('q1 ' + ids() + ' ' + (this === globalThis) + ' ' + arguments.length); });
Promise.resolve().then(() => out.push('p1'));
queueMicrotask(() => { out.push('q2'); queueMicrotask(() => out.push('q2-inner')); });
for (const bad of [undefined, 1, {}, 'f']) {
  try { queueMicrotask(bad); } catch (e) { out.push(e.code + ' ' + e.message); }
}
setTimeout(() => {
  const als = new ah.AsyncLocalStorage();
  als.run('S', () => queueMicrotask(() => out.push('als ' + als.getStore())));
  queueMicrotask(() => out.push('als-out ' + als.getStore()));
  const hook = ah.createHook({ init(id, type) { if (type === 'Microtask') out.push('init Microtask'); } }).enable();
  queueMicrotask(() => { out.push('hooked ' + (ah.executionAsyncId() > 0)); hook.disable(); });
}, 5);
setTimeout(() => console.log(out.join('\n')), 40);

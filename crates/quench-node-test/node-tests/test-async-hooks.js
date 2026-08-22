const hooks = require('node:async_hooks');
const bare = require('async_hooks');
if (hooks !== bare) throw new Error('async_hooks aliases must share module');
if (typeof hooks.createHook !== 'function') throw new Error('createHook export');
const resource = new hooks.AsyncResource('fixture');
if (typeof resource.asyncId !== 'function' || typeof resource.triggerAsyncId !== 'function') {
  throw new Error('AsyncResource id methods');
}
if (typeof resource.emitDestroy !== 'function' || typeof resource.bind !== 'function') {
  throw new Error('AsyncResource lifecycle methods');
}
const before = hooks.executionAsyncId();
const result = resource.runInAsyncScope(function (x) {
  if (hooks.executionAsyncId() !== resource.asyncId()) throw new Error('scope id');
  return this.value + x;
}, { value: 4 }, 3);
if (result !== 7 || hooks.executionAsyncId() !== before) throw new Error('scope restoration');
const storage = new hooks.AsyncLocalStorage();
if (typeof storage.run !== 'function' || typeof storage.getStore !== 'function') {
  throw new Error('AsyncLocalStorage methods');
}
if (storage.getStore() !== undefined) throw new Error('initial store');
resource.emitDestroy();
console.log('async_hooks: ok');

const assert = require('assert');
const async_hooks = require('async_hooks');
let n = 0;
const hook = async_hooks.createHook({init(id, type) { if (type === 'PROMISE') n++; }});
hook.enable();
new Promise((resolve) => resolve(1));
new Promise((resolve) => resolve(2));
hook.disable();
assert.strictEqual(n, 2);

const os = require('node:os');
const assert = require('assert');

// Node exposes these as functions returning real host info.
for (const fn of ['type', 'platform', 'arch', 'release', 'hostname', 'homedir', 'tmpdir']) {
  assert.strictEqual(typeof os[fn], 'function', `os.${fn} is a function`);
  assert.strictEqual(typeof os[fn](), 'string', `os.${fn}() returns a string`);
}
assert.strictEqual(typeof os.uptime(), 'number', 'os.uptime() returns a number');
assert.strictEqual(typeof os.EOL, 'string', 'os.EOL is the constant property');
assert.ok(os.type().length > 0, 'os.type() is real');
assert.ok(os.release().length > 0, 'os.release() is real');

const ifaces = os.networkInterfaces();
console.log('count:', Object.keys(ifaces).length);
console.log('keys:', Object.keys(ifaces).join(','));
for (const name of Object.keys(ifaces)) {
  for (const a of ifaces[name]) {
    console.log(name, a.family, a.address, a.internal);
  }
}

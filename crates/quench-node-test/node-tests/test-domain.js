const domain = require('node:domain');
const bare = require('domain');

if (domain !== bare) throw new Error('node:domain and domain must share the module');
if (typeof domain.Domain !== 'function') throw new Error('Domain export');
if (typeof domain.create !== 'function' || typeof domain.createDomain !== 'function') {
  throw new Error('domain factory exports');
}
const d = domain.create();
let routed = false;
d.on('error', error => {
  if (error.message !== 'domain fixture') throw error;
  routed = true;
});
if (domain.active !== null) throw new Error('domain active initial state');
d.run(() => {
  if (domain.active !== d) throw new Error('domain active state');
  throw new Error('domain fixture');
});
if (!routed || domain.active !== null) {
  throw new Error('domain error routing or cleanup');
}
console.log('domain: ok');

// Node compat: dns.lookup, resolve4, and promises surface.
const dns = require('node:dns');
dns.lookup('localhost', (err, address, family) => {
  if (err) throw err;
  if (typeof address !== 'string') throw new Error('no address');
  if (family !== 4 && family !== 6) throw new Error('family=' + family);
  console.log('dns: %s %s', address, family);
});
dns.resolve4('localhost', (err, addresses) => {
  if (err) throw err;
  if (!Array.isArray(addresses) || addresses.length === 0) throw new Error('empty resolve4');
  console.log('dns resolve4: %s', addresses[0]);
});
if (typeof dns.promises.lookup !== 'function' || typeof dns.promises.resolve4 !== 'function') {
  throw new Error('dns.promises missing');
}
dns.promises.lookup('localhost').then(r => {
  if (!Array.isArray(r) || r[1] !== 4) throw new Error('promise lookup');
  console.log('dns promises: ok');
}).catch(e => { throw e; });
if (typeof dns.promises.resolveTlsa !== 'undefined') {
  throw new Error('resolveTlsa is not implemented in this compatibility target');
}
console.log('dns resolveTlsa: documented missing');
setTimeout(() => {}, 50);
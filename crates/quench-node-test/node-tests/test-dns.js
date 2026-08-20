// Node compat: dns.lookup.
const dns = require('node:dns');
dns.lookup('localhost', (err, address, family) => {
  if (err) throw err;
  if (typeof address !== 'string') throw new Error('no address');
  if (family !== 4 && family !== 6) throw new Error('family=' + family);
  console.log('dns: %s %s', address, family);
});

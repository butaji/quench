// Node compat: require('node:mod') for every v1 module.
const expected = ['assert','buffer','console','dns','events','fs','net',
  'os','path','process','querystring','stream','timers','tty','url','util'];
for (const name of expected) {
  const m = require('node:' + name);
  if (typeof m !== 'object' && typeof m !== 'function') {
    throw new Error(name + ': bad type ' + typeof m);
  }
}
console.log('v1: %d modules', expected.length);

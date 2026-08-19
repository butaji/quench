// Node compat: sea stub (Single Executable Application).
const sea = require('node:sea');
if (typeof sea !== 'object') throw new Error('sea: ' + typeof sea);
if (typeof sea.isSea !== 'function') throw new Error('isSea: ' + typeof sea.isSea);
console.log('sea: ok');

// Node compat: sea stub (Single Executable Application).
const sea = require('node:sea');
if (typeof sea !== 'object' || sea === null) throw new Error('sea: ' + typeof sea);
if (typeof sea.isSea !== 'function') throw new Error('isSea: ' + typeof sea.isSea);
if (sea.isSea() !== false) throw new Error('isSea should be false outside a single executable');

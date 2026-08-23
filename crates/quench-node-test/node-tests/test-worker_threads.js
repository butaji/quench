// Node compat: worker_threads stub.
const wt = require('node:worker_threads');
if (typeof wt.Worker !== 'function') throw new Error('Worker: ' + typeof wt.Worker);
console.log('wt: ok');

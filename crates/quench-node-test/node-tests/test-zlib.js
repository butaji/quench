// Node compat: zlib + perf_hooks shape.
const zlib = require('node:zlib');
const perf = require('node:perf_hooks');
if (typeof zlib.gzip !== 'function') throw new Error('gzip: ' + typeof zlib.gzip);
if (typeof perf.performance !== 'object' && typeof perf.performance !== 'function') throw new Error('performance: ' + typeof perf.performance);
console.log('zlib+perf: ok');

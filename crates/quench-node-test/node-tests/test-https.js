// Node compat: https stub.
const https = require('node:https');
if (typeof https.request !== 'function') throw new Error('request: ' + typeof https.request);
if (typeof https.get !== 'function') throw new Error('get: ' + typeof https.get);
console.log('https: ok');

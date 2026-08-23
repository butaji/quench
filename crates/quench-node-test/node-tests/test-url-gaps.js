// Node compat: url.fileURLToPath and url.pathToFileURL round-trip (green).
const url = require('node:url');
if (typeof url.fileURLToPath !== 'function') throw new Error('no fileURLToPath');
if (typeof url.pathToFileURL !== 'function') throw new Error('no pathToFileURL');

const p = '/tmp/quench-url-fixture.txt';
const fileUrl = url.pathToFileURL(p);
if (!(fileUrl instanceof url.URL)) throw new Error('pathToFileURL not URL: ' + fileUrl);
if (fileUrl.protocol !== 'file:') throw new Error('protocol=' + fileUrl.protocol);
const back = url.fileURLToPath(fileUrl);
if (back !== p) throw new Error('round-trip mismatch: ' + back + ' !== ' + p);

// A plain file URL path decodes.
const decoded = url.fileURLToPath(new url.URL('file:///tmp/space%20name.txt'));
if (decoded !== '/tmp/space name.txt') throw new Error('decode=' + decoded);

console.log('url-gaps: ok');
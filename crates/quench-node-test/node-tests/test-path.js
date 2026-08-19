// Node compat: path module.
const path = require('node:path');
const joined = path.join('tmp', 'a', 'b.js');
if (!(joined === 'tmp/a/b.js')) throw new Error('join=' + joined);
const norm = path.normalize('a//b/../c');
if (!(norm === 'a/c')) throw new Error('normalize=' + norm);
const dir = path.dirname('/foo/bar.js');
if (!(dir === '/foo')) throw new Error('dirname=' + dir);
const base = path.basename('/foo/bar.js');
if (!(base === 'bar.js')) throw new Error('basename=' + base);
console.log('path: ' + joined + ' ' + norm);

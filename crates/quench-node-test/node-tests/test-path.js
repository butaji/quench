const path = require('node:path');
if (path.posix.sep !== '/') throw new Error('posix.sep');
if (path.win32.sep !== '\\') throw new Error('win32.sep');
if (path.posix.delimiter !== ':') throw new Error('posix.delim');
if (path.win32.delimiter !== ';') throw new Error('win32.delim');
if (typeof path.matchesGlob === 'function') {
  if (!path.matchesGlob('src/index.js', 'src/*.js')) throw new Error('matchesGlob positive');
  if (path.matchesGlob('src/index.js', 'lib/*.js')) throw new Error('matchesGlob negative');
}
console.log('path: %s %s', path.posix.sep, path.win32.sep);

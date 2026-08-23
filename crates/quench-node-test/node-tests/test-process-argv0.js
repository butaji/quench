// Node compat: process.argv0 (Bun-yellow process gap).
if (typeof process.argv0 !== 'string') throw new Error('argv0 missing: ' + typeof process.argv0);
if (process.argv0.length === 0) throw new Error('argv0 empty');
if (process.argv0 !== process.argv[0]) {
  throw new Error('argv0 != argv[0]: ' + process.argv0 + ' vs ' + process.argv[0]);
}
console.log('process-argv0: ok');
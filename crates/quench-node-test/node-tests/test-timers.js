// Node compat: timers + setImmediate.
let fired = [];
const id1 = setTimeout(() => fired.push('timeout'), 0);
const id2 = setImmediate(() => fired.push('immediate'));
const id3 = setInterval(() => fired.push('interval'), 1e9);
if (typeof id1 !== 'object' || !id1.hasRef()) throw new Error('setTimeout: ' + typeof id1);
if (typeof id2 !== 'object' || typeof id2.unref !== 'function')
  throw new Error('setImmediate: ' + typeof id2);
if (typeof id3 !== 'object' || !id3.hasRef()) throw new Error('setInterval: ' + typeof id3);
clearTimeout(id1);
clearImmediate(id2);
clearInterval(id3);
const id4 = setTimeout((a, b) => fired.push(a + b), 0, 'arg', 's');
id4.unref();
id4.ref();
process.on('exit', () => {
  if (fired.join(',') !== 'args') throw new Error('fired: ' + fired.join(','));
  console.log('timers: ' + fired.join(' '));
});

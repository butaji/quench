// Node compat: timers + setImmediate.
const id1 = setTimeout(() => {}, 0);
const id2 = setImmediate(() => {});
const id3 = setInterval(() => {}, 1e9);
if (typeof id1 !== 'number' || id1 <= 0) throw new Error('setTimeout: ' + id1);
if (typeof id2 !== 'number' || id2 <= 0) throw new Error('setImmediate: ' + id2);
if (typeof id3 !== 'number' || id3 <= 0) throw new Error('setInterval: ' + id3);
clearTimeout(id1);
clearImmediate(id2);
clearInterval(id3);
console.log('timers: ' + id1 + ' ' + id2 + ' ' + id3);

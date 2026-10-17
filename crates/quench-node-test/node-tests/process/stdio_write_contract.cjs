'use strict';
// process.stdout/stderr.write(chunk[, encoding][, callback]): the encoding decodes string
// chunks, the callback runs on the next tick queued at write time with `null`, empty writes
// still call back, and an unknown encoding throws before anything is written.
const order = [];
process.nextTick(() => order.push('tick before'));
const accepted = process.stdout.write('68690a', 'hex', function (...args) {
  order.push(['hex callback', args.length, args[0], this === undefined]);
});
process.stdout.write(Buffer.from('buffer\n'), (error) => order.push(['buffer callback', error]));
process.stdout.write('', () => order.push('empty stdout callback'));
process.stderr.write('', () => order.push('empty stderr callback'));
process.stdout.write('falsy encoding\n', null, () => order.push('null encoding callback'));
process.stdout.write('ignored callback\n', 'utf8', 5);
process.nextTick(() => order.push('tick after'));
Promise.resolve().then(() => order.push('microtask'));
for (const [label, run] of [
  ['unknown encoding', () => process.stdout.write('x', 'bogus')],
  ['non-chunk', () => process.stdout.write(5)],
]) {
  try {
    run();
  } catch (error) {
    order.push([label, error.name, error.code, error.message]);
  }
}
setImmediate(() => console.log(JSON.stringify(order), accepted));

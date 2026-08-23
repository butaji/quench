// Node compat: Readable.from supports iterable, async iterable, and pull sources.
const { Readable } = require('node:stream');

async function collect(readable) {
  const values = [];
  for await (const value of readable) values.push(value);
  return values;
}

(async () => {
  if (typeof Readable.from !== 'function') throw new Error('Readable.from missing');
  const array = await collect(Readable.from([1, 2, 3]));
  if (array.join(',') !== '1,2,3') throw new Error('array source: ' + array);

  const asyncValues = {
    async *[Symbol.asyncIterator]() {
      yield 'a';
      yield 'b';
    }
  };
  const asyncResult = await collect(Readable.from(asyncValues));
  if (asyncResult.join('') !== 'ab') throw new Error('async source: ' + asyncResult);

  let next = 0;
  const pullResult = await collect(Readable.from({ read() {
    return next < 2 ? ++next : null;
  }}));
  if (pullResult.join(',') !== '1,2') throw new Error('pull source: ' + pullResult);

  console.log('stream-from: ok');
})();

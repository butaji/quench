const regexp = /a/g;
regexp.lastIndex = 1;
const regexpFirst = regexp.test('ba');
regexp.compile('b', 'i');
const regexpResult = [regexp.source, regexp.flags, regexp.test('B'), regexpFirst];

const map = new WeakMap();
const key = {};
map.set(key, {value: 3});
const weakMapResult = [map.has(key), map.get(key).value, map.delete(key), map.has(key)];

const iterator = [4, 5][Symbol.iterator]();
const iteratorResult = [iterator.next(), iterator.next(), iterator.next()];

function capture() {
  let count = 2;
  return function update() {
    eval('count += 3');
    return count;
  };
}
const update = capture();
const closureResult = [update(), update()];

const duration = new Temporal.Duration(0, 0, 0, 0, 1, 2, 3, 4, 5, 6);
const zoned = Temporal.ZonedDateTime.from('2020-01-01T12:34:56.123456789+00:00[UTC]');
const temporalResult = [duration.toString(), duration.hours, zoned.offset, zoned.add({days: 1}).toString()];

console.log(JSON.stringify({regexpResult, weakMapResult, iteratorResult, closureResult, temporalResult}));

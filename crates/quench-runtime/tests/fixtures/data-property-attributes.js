'use strict';

function descriptor(object, key) {
  const d = Object.getOwnPropertyDescriptor(object, key);
  return d === undefined ? 'absent' : [d.value, d.writable, d.enumerable, d.configurable].join(':');
}

let rows = 0;
for (let index = 0; index < 128; index++) {
  const key = 'field' + index;
  for (const writable of [false, true]) {
    for (const enumerable of [false, true]) {
      for (const configurable of [false, true]) {
        const properties = { [key]: { value: index, writable, enumerable, configurable } };
        const first = Object.create(null, properties);
        const second = Object.create(null, properties);
        const before = descriptor(first, key);
        const keys = Object.keys(first).join(',');
        let assigned = 'ok';
        let redefined = 'ok';
        try { first[key] = index + 1; } catch (error) { assigned = error.name; }
        try { Object.defineProperty(first, key, { enumerable: !enumerable }); }
        catch (error) { redefined = error.name; }
        const after = descriptor(first, key);
        const deleted = Reflect.deleteProperty(first, key);
        console.log([index, writable, enumerable, configurable, before, keys, assigned,
          redefined, after, deleted, descriptor(first, key), descriptor(second, key)].join('|'));
        rows++;
      }
    }
  }
}
console.log('rows=' + rows);

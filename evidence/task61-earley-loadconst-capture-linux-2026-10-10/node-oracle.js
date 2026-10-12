'use strict';

function buildReader(initial) {
  let value = initial;
  return {
    read(marker) {
      const first = value;
      return [marker, first, value, typeof value];
    },
    update(next) {
      value = next;
    },
  };
}

function buildNested(initial) {
  const outer = initial;
  return function middle(offset) {
    const local = offset + 3;
    return function inner(marker) {
      return [marker, outer, local, outer + local];
    };
  };
}

const reader = buildReader(17);
const rows = [
  reader.read('before'),
  (() => {
    reader.update('changed');
    return reader.read('after');
  })(),
  buildReader(null).read('null'),
  buildReader(-0).read('negative-zero'),
  buildNested(40)(2)('nested'),
  buildNested('x')(4)('nested-string'),
];

console.log(JSON.stringify(rows));

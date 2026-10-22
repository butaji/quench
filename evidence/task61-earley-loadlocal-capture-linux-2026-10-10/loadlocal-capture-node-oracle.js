'use strict';

function makeReader(initial) {
  let captured = initial;
  return {
    read(input) {
      var plain = input + 2;
      return [plain + captured, plain, captured, typeof plain, typeof captured];
    },
    update(next) {
      captured = next;
    },
  };
}

const reader = makeReader(17);
const rows = [reader.read(5)];
reader.update('changed');
rows.push(reader.read('local'));
rows.push(makeReader(null).read(4));
rows.push(makeReader(-0).read(0));
console.log(JSON.stringify(rows));

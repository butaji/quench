'use strict';
function makeAdder(captured) {
  return function run(condition, input) {
    var local = input;
    if (condition) return local + captured;
    return 'skipped';
  };
}
const add = makeAdder(5);
const cases = [
  [true, 3], [true, 'value'], [true, null], [true, {}],
  [false, 9], [0, 11], ['', 13], [null, 17], [undefined, 19],
];
console.log(JSON.stringify(cases.map(([condition, input]) => add(condition, input))));

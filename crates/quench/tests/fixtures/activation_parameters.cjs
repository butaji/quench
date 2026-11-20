'use strict';
const output = [];
function empty() {}
function unused(a, b, c, d, e, f) { return 42; }
function probe(a, b, c, d, e, f) {
  let local;
  if (a) local = b;
  gc();
  return [a, b, c, d, e, f, local].map(value =>
    [typeof value, value === undefined, typeof value === 'object' && value !== null ? value.id : value]);
}
for (let round = 0; round < 40; round++) {
  output.push(empty() === undefined, unused(1, 2, 3, 4, 5, 6, 7));
  probe(true, { id: round }, 'poison', 4, 5, 6);
  output.push(probe());
  output.push(probe(false));
  output.push(probe(false, 'B'));
  output.push(probe(false, 'B', 'C'));
  output.push(probe(false, 'B', 'C', 'D'));
}
function recursive(depth, a, b) {
  let local;
  if (depth) {
    local = a;
    return recursive(depth - 1, b);
  }
  gc();
  return [typeof a, typeof b, typeof local];
}
output.push(recursive(7, { id: 'root' }, 'value'));
const marker = {};
function fail(a, b, c) { const local = [a, b, c]; gc(); throw marker; }
try { fail('A', 'B', 'C'); } catch (error) { output.push(error === marker); }
output.push(probe(false));
console.log(JSON.stringify(output));

'use strict';
function show(value) {
  if (typeof value === 'bigint') return `${value}n`;
  if (typeof value === 'number' && Number.isNaN(value)) return 'NaN';
  if (typeof value === 'number' && Object.is(value, -0)) return '-0';
  return value;
}
const rows = [];
function keepAnd(left, right, marker) { return (left + right) && marker; }
function keepOr(left, right, marker) { return (left * right) || marker; }
function keepComparison(left, right, marker) { return (left === right) && marker; }
function keepRelation(left, right, marker) { return (left < right) || marker; }
for (const [name, left, right] of [
  ['sum-zero', 0, 0],
  ['sum-empty', '', ''],
  ['sum-negative-zero', -0, -0],
  ['sum-nan', NaN, 1],
  ['sum-bigint-zero', 0n, 0n],
  ['sum-bigint-positive', 20n, 22n],
  ['sum-string', 'a', 'b'],
]) rows.push([name, show(keepAnd(left, right, 'right'))]);
for (const [name, left, right] of [
  ['product-zero', 0, 9],
  ['product-negative-zero', -0, 3],
  ['product-bigint-zero', 2n, 0n],
  ['product-positive', 6, 7],
]) rows.push([name, show(keepOr(left, right, 'fallback'))]);
rows.push(['eq-false', show(keepComparison({x: 1}, {x: 1}, 'equal'))]);
rows.push(['eq-true', show(keepComparison(42, 42, 'equal'))]);
rows.push(['lt-false', show(keepRelation(5, 2, 'fallback'))]);
rows.push(['lt-true', show(keepRelation(1, 2, 'fallback'))]);
const events = [];
const leftObject = { [Symbol.toPrimitive](hint) { events.push(`left:${hint}`); return 0; } };
const rightObject = { [Symbol.toPrimitive](hint) { events.push(`right:${hint}`); return 0; } };
rows.push(['coercion-zero', show(keepAnd(leftObject, rightObject, 'right'))]);
function HasInstance() {}
Object.defineProperty(HasInstance, Symbol.hasInstance, {
  value(value) { events.push(`instanceof:${value.tag}`); return false; },
});
rows.push(['instanceof-false', show(({tag: 'x'}) instanceof HasInstance || 'fallback')]);
const sentinel = { name: 'sentinel' };
const throwing = { [Symbol.toPrimitive]() { throw sentinel; } };
let thrownIdentity = false;
try { keepAnd(throwing, 1, 'right'); } catch (error) { thrownIdentity = error === sentinel; }
rows.push(['thrown-identity', thrownIdentity]);
rows.push(['events', events]);
console.log(JSON.stringify(rows));

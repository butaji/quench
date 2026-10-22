const values = [
  0, -0, 1, -1, 0.5, -0.5, Number.MIN_VALUE, Number.MAX_VALUE,
  NaN, Infinity, -Infinity, Number.MAX_SAFE_INTEGER, 2 ** 53,
  '2', 'x', true, false, null, undefined, 1n, -1n, Symbol('s')
];
function signature(value) {
  if (typeof value === 'number') {
    if (Number.isNaN(value)) return 'number:NaN';
    if (Object.is(value, -0)) return 'number:-0';
    return 'number:' + String(value);
  }
  if (typeof value === 'string') return 'string:' + JSON.stringify(value);
  if (typeof value === 'bigint') return 'bigint:' + String(value);
  if (typeof value === 'symbol') return 'symbol:' + String(value);
  return typeof value + ':' + String(value);
}
const results = [];
for (let i = 0; i < values.length; i++) {
  for (let j = 0; j < values.length; j++) {
    for (const op of ['+', '*']) {
      try {
        results.push([i, j, op, signature(op === '+' ? values[i] + values[j] : values[i] * values[j])]);
      } catch (error) {
        results.push([i, j, op, 'throw:' + error.name]);
      }
    }
  }
}
console.log('NUMBER_OPERATOR_ORACLE:' + JSON.stringify(results));

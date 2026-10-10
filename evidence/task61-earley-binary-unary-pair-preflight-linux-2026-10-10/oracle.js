function record(name, fn) {
  try { return [name, 'return', fn()]; }
  catch (e) { return [name, 'throw', e.name, e.message]; }
}
const rows = [];
rows.push(record('number_truthy_chain', () => {
  const left = 5;
  const right = 2;
  let result;
  if (-(left + right)) result = 'truthy';
  else result = 'falsy';
  return [result, left + right, -(left + right)];
}));
rows.push(record('number_falsy_chain', () => {
  const left = '';
  const right = '';
  if (+ (left + right)) return 'truthy';
  return 'falsy';
}));
rows.push(record('coercion_order', () => {
  const trace = [];
  const left = { valueOf() { trace.push('left'); return 4; } };
  const right = { valueOf() { trace.push('right'); return 1; } };
  const value = -(left + right);
  return [value, trace];
}));
rows.push(record('unary_bigint_error', () => {
  try { if (+(1n + 2n)) return 'truthy'; return 'falsy'; }
  catch (e) { return e.name; }
}));
rows.push(record('binary_throw_identity', () => {
  const thrown = { marker: 'binary' };
  const left = { [Symbol.toPrimitive]() { throw thrown; } };
  try { if (-(left + 1)) return false; }
  catch (e) { return e === thrown; }
  return false;
}));
console.log(JSON.stringify(rows));

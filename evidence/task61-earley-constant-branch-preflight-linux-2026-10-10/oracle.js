function record(name, fn) {
  try { return [name, 'return', fn()]; }
  catch (e) { return [name, 'throw', e.name, e.message]; }
}
const rows = [];
rows.push(record('literal_truthiness', () => {
  let trace = '';
  if (false) trace += 'false|';
  if (true) trace += 'true|';
  if (0) trace += 'zero|';
  if (-0) trace += 'negative-zero|';
  if (1) trace += 'one|';
  if ('') trace += 'empty|';
  if ('x') trace += 'string|';
  if (null) trace += 'null|';
  if (0n) trace += 'bigint-zero|';
  if (1n) trace += 'bigint-one|';
  return trace;
}));
rows.push(record('branch_control_flow', () => {
  let value = 0;
  if (false) value = 100;
  else value = 7;
  if (true) value += 2;
  return value;
}));
console.log(JSON.stringify(rows));

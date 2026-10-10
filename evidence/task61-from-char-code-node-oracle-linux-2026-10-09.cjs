function units(value) {
  const result = [];
  for (let i = 0; i < value.length; i++) result.push(value.charCodeAt(i));
  return result;
}
function capture(call) {
  try { return { units: units(call()) }; }
  catch (error) { return { error: error.name }; }
}
const numbers = [0, 1, 65.9, -1, 65536.8, -65537.9, NaN, Infinity, -Infinity, -0, 4294967297, Number.MIN_VALUE];
const order = [];
const converted = { valueOf() { order.push('valueOf'); return 66.75; } };
const probes = {
  numbers: numbers.map(value => capture(() => String.fromCharCode(value))),
  empty: capture(() => String.fromCharCode()),
  multiple: capture(() => String.fromCharCode(65, 55296, 66)),
  customThis: capture(() => String.fromCharCode.call({ ignored: true }, 67)),
  converted: capture(() => String.fromCharCode(converted)),
  conversionOrder: order,
  bigint: capture(() => String.fromCharCode(1n)),
  symbol: capture(() => String.fromCharCode(Symbol('x')))
};
console.log(JSON.stringify(probes));

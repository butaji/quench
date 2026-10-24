function add(left, right) {
  return left + right;
}

for (let index = 0; index < 32768; index++) add(index, 1);

function show(value) {
  if (typeof value === "number" && Object.is(value, -0)) return "-0";
  if (typeof value === "number" && Number.isNaN(value)) return "NaN";
  return value;
}

const symbol = Symbol("sentinel");
let symbolError;
try {
  add(symbol, 1);
} catch (error) {
  symbolError = error.name;
}

console.log(JSON.stringify({
  negativeZero: show(add(-0, -0)),
  nan: show(add(NaN, 1)),
  int32Overflow: add(2147483647, 1),
  stringFallback: add("value:", 4),
  symbolError,
}));

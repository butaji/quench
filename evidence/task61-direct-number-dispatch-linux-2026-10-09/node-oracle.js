const values = [
  0, -0, 1, -1, 1.5, -2.5, 2 ** 31 - 1, 2 ** 31, 2 ** 32 + 3,
  Number.MIN_VALUE, Number.MAX_VALUE, Infinity, -Infinity, NaN,
];
const operators = [
  ["==", (a, b) => a == b], ["!=", (a, b) => a != b],
  ["===", (a, b) => a === b], ["!==", (a, b) => a !== b],
  ["<", (a, b) => a < b], ["<=", (a, b) => a <= b],
  [">", (a, b) => a > b], [">=", (a, b) => a >= b],
  ["+", (a, b) => a + b], ["-", (a, b) => a - b],
  ["*", (a, b) => a * b], ["/", (a, b) => a / b],
  ["%", (a, b) => a % b], ["**", (a, b) => a ** b],
  ["<<", (a, b) => a << b], [">>", (a, b) => a >> b],
  [">>>", (a, b) => a >>> b], ["|", (a, b) => a | b],
  ["^", (a, b) => a ^ b], ["&", (a, b) => a & b],
];
function describe(value) {
  if (typeof value !== "number") return `${typeof value}:${String(value)}`;
  if (Number.isNaN(value)) return "number:NaN";
  if (Object.is(value, -0)) return "number:-0";
  if (value === Infinity) return "number:Infinity";
  if (value === -Infinity) return "number:-Infinity";
  return `number:${String(value)}`;
}
const output = [];
for (const [name, operation] of operators) {
  for (const left of values) {
    for (const right of values) {
      output.push(`${name}:${describe(operation(left, right))}`);
    }
  }
}
console.log(output.join("\n"));

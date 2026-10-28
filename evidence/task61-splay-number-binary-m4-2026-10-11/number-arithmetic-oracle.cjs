const pairs = [
  [0, 0], [1, -1], [-0, 1], [1.5, -1.5],
  [2147483647, 1], [2147483648, -1], [-2147483648, -1],
  [-2147483649, 1], [9007199254740991, 1], [-9007199254740991, -1],
  [NaN, 0], [Infinity, 2], [-Infinity, 2], [0, -0], [2, NaN],
];
const operators = [
  ["+", (a, b) => a + b], ["-", (a, b) => a - b],
  ["*", (a, b) => a * b], ["/", (a, b) => a / b],
  ["%", (a, b) => a % b], ["**", (a, b) => a ** b],
  ["<<", (a, b) => a << b], [">>", (a, b) => a >> b],
  [">>>", (a, b) => a >>> b], ["|", (a, b) => a | b],
  ["^", (a, b) => a ^ b], ["&", (a, b) => a & b],
];
function repr(value) {
  if (Number.isNaN(value)) return "NaN";
  if (Object.is(value, -0)) return "-0";
  return String(value);
}
const output = [];
for (const [token, apply] of operators) {
  for (const [left, right] of pairs) {
    output.push(`${token}\t${repr(left)}\t${repr(right)}\t${repr(apply(left, right))}`);
  }
}
console.log(output.join("\n"));

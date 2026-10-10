const rows = [];

function arithmetic(leftInput, rightInput) {
  let left = leftInput;
  let right = rightInput;
  const sum = left + right;
  const difference = left - right;
  const reversed = right - left;
  const product = left * right;
  const less = left < right;
  left = sum;
  return [sum, difference, reversed, product, less, left];
}

for (const [name, left, right] of [
  ['integers', 19, 4],
  ['negative', -9, 3],
  ['zero', 0, -0],
  ['strings', 'quench', 'vm'],
  ['coercion', '6', 2],
  ['large', 2147483648, 13],
]) {
  rows.push([name, arithmetic(left, right)]);
}

function fieldRight(value, object) {
  let local = value;
  return [local + object.value, local - object.value];
}

function fieldLeft(object, value) {
  let local = value;
  return [object.value + local, object.value - local];
}

const object = { value: 8 };
rows.push(['field-right', fieldRight(11, object)]);
rows.push(['field-left', fieldLeft(object, 5)]);

const output = JSON.stringify(rows);
if (typeof console !== 'undefined' && console.log) console.log(output);
else print(output);

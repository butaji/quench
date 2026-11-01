function accumulate(input, count) {
  let total = input + 0.25;
  for (let index = 0; index < count; index++) {
    if (total === null) return 0;
    const next = total + 0.5;
    total = next * 1.25 - 0.75;
  }
  return total;
}

function scratch(input) {
  let index = 1;
  index++;
  const next = index + input;
  input = next;
  return index + input;
}

function mixed(input, flag) {
  let value = input + 0.25;
  if (flag) value = true;
  return value;
}

function preserve(input, flag) {
  let value = input / 1;
  if (flag) value = value / 1;
  return value;
}

function floatingBranch(input) {
  const value = input / 2;
  let result = 0;
  if (value > 0.25) result = 1;
  return result;
}

globalThis.coercions = 0;
globalThis.coercing = {
  valueOf() { globalThis.coercions++; return 3; }
};
globalThis.textInput = "3";
globalThis.bigInput = 3n;
globalThis.countCoercions = 0;
globalThis.counting = {
  valueOf() { globalThis.countCoercions++; return 2; }
};

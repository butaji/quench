function localValue(flag) {
  let value = 10;
  if (flag) value += 2;
  return value;
}

function makeCounter(start) {
  let value = start;
  return function increment() {
    value += 1;
    return value;
  };
}

const counter = makeCounter(0);
for (let index = 0; index < 32768; index++) {
  localValue(index & 1);
  counter();
}

const trace = [localValue(false), localValue(true), counter()];
try {
  {
    trace.push(uninitialized);
    let uninitialized = 1;
  }
} catch (error) {
  trace.push(error.name);
}
console.log(JSON.stringify(trace));

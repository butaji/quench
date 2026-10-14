const observations = [];
function run(returnValue) {
  const iterator = {
    [Symbol.iterator]() { return this; },
    next() { return { value: 1, done: false }; },
    return() { observations.push('return'); return returnValue; }
  };
  try { for (const value of iterator) break; }
  catch (error) { observations.push(error.name); }
}
run({ done: true });
run(42);
console.log('ITERATOR_CLOSE:' + JSON.stringify(observations));

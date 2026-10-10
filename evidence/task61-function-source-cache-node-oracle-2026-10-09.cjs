function outer() {
  return function inner(value) {
    return value + 1;
  };
}

const first = outer();
const second = outer();
console.log(
  JSON.stringify({
    freshFunctionIdentity: first !== second,
    sameSourceText: first.toString() === second.toString(),
    sameFunctionName: first.name === second.name,
    firstName: first.name,
    secondName: second.name,
    source: first.toString(),
    firstResult: first(4),
    secondResult: second(9),
  }),
);

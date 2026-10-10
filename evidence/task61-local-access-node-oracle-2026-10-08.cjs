function plainLocalWrites() {
  var last = 0;
  var sum = 0;
  for (var outer = 0; outer < 40; outer++) {
    for (var index = 0; index < 250; index++) {
      last = index;
      sum += index;
    }
  }
  return [last, sum];
}

function capturedLocal() {
  var value = 4;
  function increment() { value++; }
  increment();
  return value;
}

function directEvalLocal() {
  var value = 1;
  eval('value = 3');
  return value;
}

function mappedArgument(parameter) {
  arguments[0] = 9;
  return parameter;
}

function lexicalTdz() {
  try {
    return value;
  } catch (error) {
    return error instanceof ReferenceError;
  }
  let value = 2;
}

function namedFunctionBinding() {
  var value = function self() { return self; };
  return value() === value;
}

function resolvedNamePromotesFrame() {
  const value = function self() {
    var local = 1;
    with ({}) { self = 2; }
    return local;
  };
  return value();
}

console.log(JSON.stringify({
  plain: plainLocalWrites(),
  captured: capturedLocal(),
  eval: directEvalLocal(),
  mapped: mappedArgument(1),
  tdz: lexicalTdz(),
  self: namedFunctionBinding(),
  resolvedName: resolvedNamePromotesFrame(),
}));

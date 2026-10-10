function increment(value) { return value + 1; }
function callThrough(fn, value) { return fn(value); }
console.log(callThrough(increment, 4));

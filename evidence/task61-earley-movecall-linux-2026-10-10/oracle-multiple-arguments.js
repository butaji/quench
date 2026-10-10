function add(first, second) { return first + second; }
function callThrough(fn, first, second) { return fn(first, second); }
console.log(callThrough(add, 3, 7));

// Spread in function calls
function sum(a, b, c) { return a + b + c; }
const args = [1, 2, 3];
sum(...args) === 6;

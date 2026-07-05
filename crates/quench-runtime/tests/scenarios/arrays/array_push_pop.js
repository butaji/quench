// Array push and pop
// ECMA-262 sec-array.prototype.push, sec-array.prototype.pop

let arr = [];

// Push and verify
arr.push(1);
arr.push(2);
arr.push(3);

// Test length after push
let lenAfterPush = arr.length;

// Pop and verify
let first = arr.pop();
let second = arr.pop();

// Test length after pop
let lenAfterPop = arr.length;

// Final state
arr.length;

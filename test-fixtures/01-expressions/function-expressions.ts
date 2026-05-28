// ============================================================================
// FUNCTION EXPRESSIONS
// ============================================================================

// Function declaration
function greet(name: string): string {
  return `Hello, ${name}!`;
}

// Function expression
const add = function(a: number, b: number): number {
  return a + b;
};

// Arrow function
const multiply = (a: number, b: number): number => a * b;

// Arrow with block body
const divide = (a: number, b: number): number => {
  if (b === 0) throw new Error("Division by zero");
  return a / b;
};

// Arrow with implicit return
const square = (x: number) => x * x;
const sayHi = (name: string) => `Hi, ${name}!`;

// Default parameters
function withDefaults(a: number, b: number = 10, c: number = 5) {
  return a + b + c;
}

// Rest parameters
function sumAll(...numbers: number[]): number {
  return numbers.reduce((a, b) => a + b, 0);
}

// Destructuring parameters
function withDestruct({ x, y }: { x: number; y: number }) {
  return x + y;
}

// Function calling with spread
const nums = [1, 2, 3];
const applied = Math.max(...nums);

// Higher-order functions
const double = (x: number) => x * 2;
const numbers = [1, 2, 3, 4, 5];
const doubled = numbers.map(double);
const filtered = numbers.filter(n => n > 2);
const reduced = numbers.reduce((a, b) => a + b, 0);

// Callback pattern
function processWithCallback(n: number, callback: (result: number) => void) {
  callback(n * 2);
}

// Closures
function createCounter() {
  let count = 0;
  return () => ++count;
}
const counter = createCounter();

// Recursive function
function factorial(n: number): number {
  if (n <= 1) return 1;
  return n * factorial(n - 1);
}

// IIFE (Immediately Invoked Function Expression)
const result = (function(x) {
  return x * 2;
})(5);

// Method as object property
const obj = {
  value: 10,
  getValue() { return this.value; },
  setValue(v: number) { this.value = v; }
};

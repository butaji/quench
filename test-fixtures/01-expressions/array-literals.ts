// ============================================================================
// ARRAY EXPRESSIONS
// ============================================================================

// Empty array
const empty: number[] = [];
const empty2 = [];

// With elements
const numbers = [1, 2, 3, 4, 5];
const strings = ["a", "b", "c"];
const mixed = [1, "two", true, null];

// Spread operator
const arr1 = [1, 2, 3];
const arr2 = [4, 5, 6];
const combined = [...arr1, ...arr2];
const prepend = [0, ...arr1];
const append = [...arr1, 7];

// Array with spread in expression
const copy = [...arr1];
const nested = [[1, 2], [3, 4]];

// Accessing elements
const first = numbers[0];
const last = numbers[numbers.length - 1];

// Array methods (built-in, should work at runtime)
const doubled = numbers.map(x => x * 2);
const evens = numbers.filter(x => x % 2 === 0);
const sum = numbers.reduce((acc, x) => acc + x, 0);
const found = numbers.find(x => x > 3);
const hasThree = numbers.includes(3);

// Destructuring
const [first, second, ...rest] = numbers;

// Sparse arrays
const sparse = [1, , 3];  // has a hole at index 1
const sparse2 = new Array(3);  // length 3, all undefined

// Array constructor
const fromConstructor = new Array(5);
const withValues = new Array(1, 2, 3);
const of = Array.of(1, 2, 3);

// Array.isArray
const isArr = Array.isArray(numbers);
const isNotArr = Array.isArray("not array");

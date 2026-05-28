// ============================================================================
// DESTRUCTURING - Arrays
// ============================================================================

// Basic array destructuring
const [a, b, c] = [1, 2, 3];

// Skipping elements
const [first, , third] = [1, 2, 3];

// Rest pattern
const [head, ...tail] = [1, 2, 3, 4, 5];
const [first, ...rest] = [1];

// Default values
const [x = 0, y = 0, z = 0] = [1, 2];
const [a1 = "default"] = [];

// Nested destructuring
const [[nested], [a, b]] = [[1], [2, 3]];

// With functions
function firstTwo([a, b]: [number, number, ...number[]]): [number, number] {
  return [a, b];
}

// Swap variables
let left = 1;
let right = 2;
[left, right] = [right, left];

// Destructuring in function parameters
const sum = ({ a, b }: { a: number; b: number }) => a + b;

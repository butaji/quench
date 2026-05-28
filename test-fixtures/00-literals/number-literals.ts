// ============================================================================
// NUMBER OPERATIONS
// ============================================================================

// Basic arithmetic
const add = 1 + 2;
const sub = 5 - 3;
const mul = 4 * 3;
const div = 10 / 2;
const mod = 10 % 3;
const exp = 2 ** 4;

// With variables
let a = 10;
let b = 3;
const sum = a + b;
const diff = a - b;
const product = a * b;
const quotient = a / b;
const remainder = a % b;

// Assignment operators
let x = 10;
x += 5;  // 15
x -= 3;  // 12
x *= 2;  // 24
x /= 4;  // 6
x %= 4;  // 2

// Increment/Decrement
let i = 0;
i++;
++i;
i--;
--i;

// Unary
const neg = -5;
const pos = +5;

// Comparison
const eq = 5 == 5;
const neq = 5 != 3;
const strictEq = 5 === 5;
const strictNeq = 5 !== 3;
const lt = 3 < 5;
const lte = 3 <= 5;
const gt = 5 > 3;
const gte = 5 >= 5;

// Special values
const infinity = Infinity;
const negInfinity = -Infinity;
const nan = NaN;
const maxSafe = Number.MAX_SAFE_INTEGER;
const minSafe = Number.MIN_SAFE_INTEGER;

// ============================================================================
// BINARY OPERATORS
// ============================================================================

// Arithmetic
const add = 5 + 3;
const sub = 5 - 3;
const mul = 5 * 3;
const div = 15 / 3;
const mod = 17 % 5;
const exp = 2 ** 10;

// Comparison
const eq = 5 == "5";        // loose equality
const neq = 5 != "5";
const strictEq = 5 === 5;   // strict equality
const strictNeq = 5 !== "5";
const lt = 3 < 5;
const lte = 3 <= 5;
const gt = 5 > 3;
const gte = 5 >= 5;

// Logical operators
const and = true && false;
const or = true || false;
const not = !true;

// Bitwise
const bitAnd = 0b1100 & 0b1010;  // 0b1000 = 8
const bitOr = 0b1100 | 0b1010;  // 0b1110 = 14
const bitXor = 0b1100 ^ 0b1010; // 0b0110 = 6
const bitNot = ~0b1100;          // two's complement

// Shifts
const leftShift = 1 << 3;   // 8
const rightShift = 8 >> 2;   // 2
const unsignedRight = -8 >>> 2; // unsigned right shift

// in operator
const obj = { a: 1, b: 2 };
const hasA = "a" in obj;  // true
const hasC = "c" in obj;  // false

// instanceof
class Animal {}
class Dog extends Animal {}
const dog = new Dog();
const isAnimal = dog instanceof Animal;
const isDog = dog instanceof Dog;

// typeof
const typeOfStr = typeof "hello";
const typeOfNum = typeof 42;
const typeOfBool = typeof true;
const typeOfObj = typeof {};
const typeOfArr = typeof [];
const typeOfFunc = typeof function() {};

// String concatenation
const concat = "Hello, " + "world!";
const mixedConcat = "Result: " + 42;

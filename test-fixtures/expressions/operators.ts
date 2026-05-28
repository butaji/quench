// ===========================================
// TypeScript Operator Expression Tests
// ===========================================

// Arithmetic operators
const add: number = 5 + 3;
const sub: number = 10 - 4;
const mul: number = 6 * 7;
const div: number = 20 / 4;
const mod: number = 17 % 5;
const neg: number = -42;

// Increment/Decrement
let counter: number = 0;
counter++;
const afterInc: number = counter;
counter--;
const afterDec: number = counter;

// Comparison operators
const eq: boolean = 5 === 5;
const neq: boolean = 5 !== 3;
const lt: boolean = 3 < 5;
const lte: boolean = 3 <= 5;
const gt: boolean = 5 > 3;
const gte: boolean = 5 >= 5;

// Logical operators
const and: boolean = true && false;
const or: boolean = true || false;
const not: boolean = !true;

// String operators
const concat: string = "Hello" + " " + "World";
const strEq: boolean = "foo" === "foo";

// Ternary operator
const ternary: number = true ? 1 : 0;

// Type operators
const typeOfStr = typeof "hello";
const typeOfNum = typeof 42;

// Function to validate
export function validateOperators(): boolean {
  return add === 8
    && sub === 6
    && mul === 42
    && div === 5
    && mod === 2
    && eq === true
    && neq === true
    && lt === true
    && and === false
    && or === true
    && not === false
    && ternary === 1;
}

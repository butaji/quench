// ===========================================
// TypeScript Literal Expression Tests
// ===========================================

// String literals
const str1: string = "hello";
const str2: string = 'world';
const str3: string = `template`;
const str4: string = `Hello, ${str1}!`;

// Number literals
const num1: number = 42;
const num2: number = 3.14;
const num3: number = -10;
const num4: number = 0xFF; // hex
const num5: number = 0b1010; // binary
const num6: number = 0o755; // octal

// Boolean literals
const bool1: boolean = true;
const bool2: boolean = false;

// Null and undefined
const null1: null = null;
const undef1: undefined = undefined;

// Template literals
const name: string = "TypeScript";
const greeting: string = `Hello, ${name}!`;
const multiLine: string = `
  This is a
  multi-line string
`;

// Array literal
const arr1: number[] = [1, 2, 3, 4, 5];
const arr2: string[] = ["a", "b", "c"];
const arr3: (number | string)[] = [1, "two", 3];

// Object literal
const obj1: { name: string; age: number } = { name: "John", age: 30 };
const obj2 = { x: 1, y: 2, z: 3 };

// Function to validate
export function validateLiterals(): boolean {
  return str1 === "hello"
    && num1 === 42
    && bool1 === true
    && arr1.length === 5
    && obj1.name === "John";
}

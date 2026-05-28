// ===========================================
// TypeScript Array Operation Tests
// ===========================================

// Array creation
const numbers: number[] = [1, 2, 3, 4, 5];
const strings: string[] = ["a", "b", "c"];
const mixed: (number | string)[] = [1, "two", 3];

// Array access
const first: number = numbers[0];
const last: number = numbers[numbers.length - 1];

// Array methods
const doubled: number[] = numbers.map((n) => n * 2);
const evens: number[] = numbers.filter((n) => n % 2 === 0);
const sum: number = numbers.reduce((acc, n) => acc + n, 0);
const found: number | undefined = numbers.find((n) => n > 3);
const foundIndex: number = numbers.findIndex((n) => n > 3);
const hasThree: boolean = numbers.includes(3);
const anyEven: boolean = numbers.some((n) => n % 2 === 0);
const allPositive: boolean = numbers.every((n) => n > 0);

// Array spread
const combined: number[] = [...numbers, 6, 7, 8];
const copy: number[] = [...numbers];

// Array destructuring
const [firstNum, secondNum, ...rest] = numbers;

// Nested arrays
const matrix: number[][] = [[1, 2], [3, 4], [5, 6]];
const matrixElement: number = matrix[1][1];

// Function to validate
export function validateArrays(): boolean {
  return first === 1
    && doubled[0] === 2
    && evens.length === 2
    && sum === 15
    && found === 4
    && combined.length === 8;
}

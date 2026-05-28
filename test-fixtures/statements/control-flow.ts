// ===========================================
// TypeScript Control Flow Tests
// ===========================================

// If-else statements
function testIfElse(x: number): string {
  if (x > 0) {
    return "positive";
  } else if (x < 0) {
    return "negative";
  } else {
    return "zero";
  }
}

// While loop
function sumWhile(n: number): number {
  let sum = 0;
  let i = 0;
  while (i <= n) {
    sum += i;
    i++;
  }
  return sum;
}

// For loop
function sumFor(n: number): number {
  let sum = 0;
  for (let i = 0; i <= n; i++) {
    sum += i;
  }
  return sum;
}

// For-of loop
function concatStrings(arr: string[]): string {
  let result = "";
  for (const item of arr) {
    result += item;
  }
  return result;
}

// Do-while loop
function doWhileTest(): number {
  let count = 0;
  do {
    count++;
  } while (count < 5);
  return count;
}

// Switch statement
function testSwitch(x: number): string {
  switch (x) {
    case 1:
      return "one";
    case 2:
      return "two";
    case 3:
      return "three";
    default:
      return "other";
  }
}

// Break and continue
function testBreakContinue(n: number): number {
  let sum = 0;
  for (let i = 0; i < n; i++) {
    if (i === 5) {
      break;
    }
    if (i % 2 === 0) {
      continue;
    }
    sum += i;
  }
  return sum;
}

// Return in different positions
function testReturn(): { a: number; b: number } {
  if (true) {
    return { a: 1, b: 2 };
  }
  return { a: 0, b: 0 };
}

export function validateControlFlow(): boolean {
  return testIfElse(5) === "positive"
    && testIfElse(-3) === "negative"
    && testIfElse(0) === "zero"
    && sumWhile(5) === 15
    && sumFor(5) === 15
    && concatStrings(["a", "b", "c"]) === "abc"
    && doWhileTest() === 5
    && testSwitch(2) === "two"
    && testBreakContinue(10) === 9;
}

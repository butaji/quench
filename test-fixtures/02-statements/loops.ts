// ============================================================================
// LOOPS - for, while, do-while, for-of, for-in
// ============================================================================

// For loop
const forSum = (n: number): number => {
  let sum = 0;
  for (let i = 1; i <= n; i++) {
    sum += i;
  }
  return sum;
};

// For loop with multiple variables
const forMultiple = (arr: number[]): number => {
  let sum = 0;
  for (let i = 0, j = arr.length - 1; i < arr.length; i++, j--) {
    sum += arr[i];
  }
  return sum;
};

// While loop
const whileSum = (n: number): number => {
  let sum = 0;
  let i = 1;
  while (i <= n) {
    sum += i;
    i++;
  }
  return sum;
};

// Do-while loop
const doWhileSum = (n: number): number => {
  let sum = 0;
  let i = 1;
  do {
    sum += i;
    i++;
  } while (i <= n);
  return sum;
};

// For-of loop (iterating arrays)
const forOfSum = (arr: number[]): number => {
  let sum = 0;
  for (const num of arr) {
    sum += num;
  }
  return sum;
};

// For-of with index
const withIndex = (arr: string[]): string[] => {
  const result: string[] = [];
  let i = 0;
  for (const item of arr) {
    result.push(`${i}: ${item}`);
    i++;
  }
  return result;
};

// For-in loop (iterating object keys)
const forInKeys = (obj: Record<string, number>): string[] => {
  const keys: string[] = [];
  for (const key in obj) {
    keys.push(key);
  }
  return keys;
};

// Break and continue
const breakExample = (arr: number[]): number => {
  for (const num of arr) {
    if (num === 0) break;
  }
  return arr.length;
};

const continueExample = (arr: number[]): number => {
  let count = 0;
  for (const num of arr) {
    if (num < 0) continue;
    count++;
  }
  return count;
};

// Nested loops with labels
const labelExample = (): number => {
  let result = 0;
  outer:
  for (let i = 0; i < 5; i++) {
    for (let j = 0; j < 5; j++) {
      if (i === 2 && j === 2) break outer;
      result++;
    }
  }
  return result;
};

// forEach (method)
const forEachSum = (arr: number[]): number => {
  let sum = 0;
  arr.forEach(n => { sum += n; });
  return sum;
};

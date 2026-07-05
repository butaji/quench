// spec: ECMA-262 sec-for-in-and-for-of-statements-runtime-semantics
// expect: value: 6
// tags: statements, for-of

const arr = [1, 2, 3];
let sum = 0;
for (const x of arr) {
  sum += x;
}
sum;

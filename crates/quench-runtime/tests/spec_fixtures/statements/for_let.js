// spec: ECMA-262 sec-let-and-const-declarations-runtime-semantics
// expect: value: 3
// tags: statements, let, scoping

let sum = 0;
for (let i = 0; i < 3; i++) {
  let x = i * 2;
  sum += x;
}
sum;

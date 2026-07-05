// spec: ECMA-262 sec-break-statement-runtime-semantics
// expect: value: 3
// tags: statements, break, label

let result = 0;
outer: for (let i = 0; i < 3; i++) {
  for (let j = 0; j < 3; j++) {
    if (i === 1 && j === 1) {
      break outer;
    }
    result++;
  }
}
result;

// spec: ECMA-262 sec-continue-statement-runtime-semantics
// expect: value: 6
// tags: statements, continue, label

let count = 0;
outer: for (let i = 0; i < 3; i++) {
  for (let j = 0; j < 3; j++) {
    if (j === 1) {
      continue outer;
    }
    count++;
  }
}
count;

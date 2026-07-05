// spec: ECMA-262 sec-do-while-statement-runtime-semantics
// expect: value: 5
// tags: statements, do-while

let i = 0;
do {
  i++;
} while (i < 5);
i;

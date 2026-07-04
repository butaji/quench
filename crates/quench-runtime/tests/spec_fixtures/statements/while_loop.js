// spec: ecma-262 sec-while-statement-runtime-semantics
// expect: value: 20
// tags: while, iteration

var i = 0;
while (i < 20) {
  i = i + 1;
}
i;

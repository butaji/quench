// spec: ECMA-262 sec-function-definitions-static-semantics-early-errors
// expect: value: [2, 3, 4]
// tags: functions, rest

function sum(...nums) {
  return nums;
}
sum(2, 3, 4);

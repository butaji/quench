// spec: ECMA-262 sec-arrow-function-definitions-runtime-semantics
// expect: value: 42
// tags: functions, arrow, this

const obj = {
  x: 42,
  method: () => this.x
};
obj.method();

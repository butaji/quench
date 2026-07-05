// spec: ECMA-262 sec-function.prototype.apply
// expect: value: 42
// tags: functions, apply

function getX() {
  return this.x;
}
const obj = { x: 42 };
getX.apply(obj);

// spec: ECMA-262 sec-function.prototype.call
// expect: value: 42
// tags: functions, call

function getX() {
  return this.x;
}
const obj = { x: 42 };
getX.call(obj);

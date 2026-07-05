// spec: ECMA-262 sec-function.prototype.bind
// expect: value: 42
// tags: functions, bind

function getX() {
  return this.x;
}
const obj = { x: 42 };
const bound = getX.bind(obj);
bound();

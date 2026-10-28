var trace = [];
var conversion = {
  [Symbol.toPrimitive]: function (hint) {
    trace.push("key:" + hint);
    return "x";
  },
};
var record = {
  get x() {
    trace.push("get");
    return 4;
  },
  set x(value) {
    trace.push("set:" + value);
  },
};
function base() {
  trace.push("base");
  return record;
}
function key() {
  trace.push("expression");
  return conversion;
}
function rhs() {
  trace.push("rhs");
  return 3;
}

base()[key()] += rhs();
print(trace.join(","));

var proxyTrace = [];
var prototype = {
  get 0() {
    proxyTrace.push("getter");
    return 4;
  },
  set 0(value) {
    proxyTrace.push("inherited-setter:" + value);
  },
};
var proxyPrototype = new Proxy(prototype, {
  set: function (target, property, value, receiver) {
    proxyTrace.push("proxy-set:" + typeof property + ":" + property + ":" + value);
    return Reflect.set(target, property, value, receiver);
  },
});
var array = [];
Object.setPrototypeOf(array, proxyPrototype);
array[0] += 1;
print(proxyTrace.join(","));

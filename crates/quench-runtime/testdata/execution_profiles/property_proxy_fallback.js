function read(object) { return object.value; }

var calls = 0;
var receiver = new Proxy({ value: 41 }, {
  get: function get(target, key, object) {
    calls++;
    return Reflect.get(target, key, object) + 1;
  }
});
var result = read(receiver);
if (result !== 42 || calls !== 1) {
  throw new Error("proxy get fallback mismatch");
}
return result;

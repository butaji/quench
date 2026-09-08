function read(object) { return object.value; }

var calls = 0;
var receiver = new Proxy({ value: 41 }, {
  get: function get(target, key, object) {
    calls++;
    return Reflect.get(target, key, object) + 1;
  }
});
function run() {
  return read(receiver);
}
function verify(result) {
if (result !== 42 || calls !== 1) {
  throw new Error("proxy get fallback mismatch");
}
  return result;
}
return { run: run, verify: verify };

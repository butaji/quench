function read(object) {
  return object.value;
}

var calls = 0;
var receiver = {};
Object.defineProperty(receiver, "value", {
  get: function getValue() {
    calls++;
    throw new Error("boom");
  }
});
function run() {
  try {
    read(receiver);
  } catch (error) {
    return error.message + ":" + calls;
  }
  return "missing throw:" + calls;
}
function verify(result) {
if (result !== "boom:1") {
  throw new Error("accessor throw/replay mismatch: " + result);
}
  return result;
}
return { run: run, verify: verify };

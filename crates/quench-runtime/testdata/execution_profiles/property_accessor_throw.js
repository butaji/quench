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
var result;
try {
  read(receiver);
} catch (error) {
  result = error.message + ":" + calls;
}
if (result !== "boom:1") {
  throw new Error("accessor throw/replay mismatch: " + result);
}
return result;

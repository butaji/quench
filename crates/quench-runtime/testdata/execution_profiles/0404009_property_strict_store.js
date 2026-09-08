function write(object, value) {
  "use strict";
  object.value = value;
  return object.value;
}

var receiver = { value: 0 };
write(receiver, 1);
var result = write(receiver, 42);
if (result !== 42 || receiver.value !== 42) {
  throw new Error("strict own-property store mismatch");
}
return result;

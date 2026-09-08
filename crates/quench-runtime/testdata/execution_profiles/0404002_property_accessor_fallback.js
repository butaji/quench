function read(object) {
  return object.value;
}

var receiver = { value: 19 };
read(receiver);
read(receiver);
Object.defineProperty(receiver, "value", { get: undefined });
function run() {
  return read(receiver);
}
function verify(result) {
if (result !== undefined) {
  throw new Error("property accessor fallback mismatch");
}
  return result;
}
return { run: run, verify: verify };

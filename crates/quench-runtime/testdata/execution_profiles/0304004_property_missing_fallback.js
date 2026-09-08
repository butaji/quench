function read(object) {
  return object.missing;
}

read({ value: 1 });
var receiver = { value: 2 };
function verify(result) {
  if (read(receiver) !== undefined) throw new Error("missing-property mismatch");
  if (result !== undefined) throw new Error("measured missing-property mismatch");
  return result;
}
return { run: read, verify: verify, arguments: [receiver] };

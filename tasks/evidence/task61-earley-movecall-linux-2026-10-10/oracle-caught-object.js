const marker = {};
function throwValue(value) { throw value; }
function catchCall(fn, value) {
  try { fn(value); return false; }
  catch (error) { return error === value; }
}
console.log(catchCall(throwValue, marker));

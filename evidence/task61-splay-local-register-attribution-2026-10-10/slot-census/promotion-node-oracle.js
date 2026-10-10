function addPair(left, right) { return left + right; }
function missing(value) { return value === undefined ? "undefined" : value; }
function mapped(value) { arguments[0] = 11; return [value, arguments[0], arguments.length]; }
function mutate(value) { const before = value; value += 3; return [before, value]; }
function postIncrement(value) { return value++; }
function closeOver(value) { return () => value; }
const deleteParameter = Function("value", "return delete value;");
function evalParameter(value) { eval("value = value"); return value; }

console.log(JSON.stringify([
  addPair(4, 5),
  missing(),
  mapped(7, 8),
  mutate(9),
  postIncrement(10),
  closeOver(12)(),
  deleteParameter(13),
  evalParameter(14),
]));

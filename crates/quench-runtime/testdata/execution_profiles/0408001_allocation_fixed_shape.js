function Point(x, y) {
  this.x = x;
  this.y = y;
}

function create(x, y) {
  var point = new Point(x, y);
  return point.x + point.y;
}

create(1, 2);
function run() {
  return create(19, 23);
}
function verify(result) {
if (result !== 42) {
  throw new Error("fixed-shape allocation mismatch");
}
  return result;
}
return { run: run, verify: verify };

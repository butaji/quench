function Point(x, y) {
  this.x = x;
  this.y = y;
}

function create(x, y) {
  var point = new Point(x, y);
  return point.x + point.y;
}

create(1, 2);
var result = create(19, 23);
if (result !== 42) {
  throw new Error("fixed-shape allocation mismatch");
}
return result;

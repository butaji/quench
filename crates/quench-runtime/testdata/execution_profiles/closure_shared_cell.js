function makePair() {
  var value = 0;
  return [
    function increment() { value++; return value; },
    function read() { return value; }
  ];
}

var pair = makePair();
pair[0]();
pair[0]();
var result = pair[1]();
if (result !== 2) {
  throw new Error("shared closure cell mismatch");
}
return result;

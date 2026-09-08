function makePair() {
  var value = 0;
  return [
    function increment() { value++; return value; },
    function read() { return value; }
  ];
}

var pair = makePair();
function run() {
  pair[0]();
  pair[0]();
  return pair[1]();
}
function verify(result) {
if (result !== 2) {
  throw new Error("shared closure cell mismatch");
}
  return result;
}
return { run: run, verify: verify };

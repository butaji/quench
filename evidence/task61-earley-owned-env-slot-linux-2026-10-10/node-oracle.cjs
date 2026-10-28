const output = {};
function capturedParameters() {
  let outer = 7;
  return function middle(delta) {
    let value = outer + delta;
    return { read() { return value; }, bump() { return ++value; }, set(v) { value = v; } };
  };
}
const f = capturedParameters();
const a = f(3), b = f(11);
output.parameter = [a.read(), a.bump(), a.read(), b.read()];
function blockClosures() {
  const fs = [];
  for (let i = 0; i < 4; i++) {
    let value = i * 10;
    fs.push([() => value, v => value = v, () => ++value]);
  }
  return fs.map(([get, set, inc]) => { const before=get(); set(before+5); return [before,get(),inc(),get()]; });
}
output.blocks = blockClosures();
function nested() { let x = 2; return () => { let y = 5; return () => ++x + y; }; }
const nestedClosure = nested()();
output.nested = [nestedClosure(), nestedClosure()];
function argumentsCapture(...args) { return () => [args.length, args[0], ++args[1]]; }
output.arguments = argumentsCapture('x', 4)();
console.log(JSON.stringify(output));

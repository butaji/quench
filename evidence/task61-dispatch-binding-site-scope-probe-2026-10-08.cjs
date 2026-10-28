const withObject = { x: 'with' };
const result = {};
with (withObject) {
  let x = 'block';
  result.withRead = x;
  result.withDelete = delete x;
}
result.withOwnAfterDelete = Object.hasOwn(withObject, 'x');

const closureObject = { x: 'with' };
let closure;
with (closureObject) {
  let x = 'block';
  closure = function () { return eval('x'); };
}
result.withClosureEval = closure();

function directEvalSites() {
  let x = 'outer';
  function nested() {
    let x = 'inner';
    return eval('x');
  }
  return [eval('x'), nested(), eval('x')];
}
result.directEval = directEvalSites();
console.log(JSON.stringify(result));

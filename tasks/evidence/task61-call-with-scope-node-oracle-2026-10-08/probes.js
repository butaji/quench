var results = {};
function record(name, fn) {
  try {
    results[name] = { value: fn() };
  } catch (error) {
    results[name] = { throw: error.name, message: error.message };
  }
}
record('function-declaration', function () {
  var saved;
  with ({ marker: 'declaration' }) {
    function innerDeclaration() { return marker; }
    saved = innerDeclaration;
  }
  return saved();
});
record('function-expression', function () {
  var saved;
  with ({ marker: 'expression' }) {
    saved = function () { return marker; };
  }
  return saved();
});
record('direct-eval-closure', function () {
  var saved;
  with ({ marker: 'direct-eval' }) {
    saved = eval('(function () { return marker; })');
  }
  return saved();
});
record('nested-with-closure', function () {
  var saved;
  with ({ outerName: 'outer', sharedName: 'outer' }) {
    with ({ innerName: 'inner', sharedName: 'inner' }) {
      saved = function () { return [innerName, sharedName, outerName]; };
    }
  }
  return saved();
});
record('generator-created-in-with-resumed-outside', function () {
  var iterator;
  with ({ marker: 'generator' }) {
    iterator = (function* () { yield marker; yield marker + '-again'; })();
  }
  return [iterator.next().value, iterator.next().value, iterator.next().done];
});
record('throw-from-with-closure', function () {
  var saved;
  with ({ marker: 'closure-throw' }) {
    saved = function () { throw new TypeError(marker); };
  }
  try {
    saved();
    return 'not thrown';
  } catch (error) {
    return [error.name, error.message];
  }
});
console.log(JSON.stringify(results));

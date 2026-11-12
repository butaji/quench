//! By-name reads from code a direct eval compiled: the lane keeps them as hinted global reads
//! while no scope can hold the name, and every scope change that could must retire them.

use super::subset_tests::output;

const SCOPE_CHANGES: &str = r####"
// By-name loads from eval-compiled code: adversarial scope changes.
var log = [];
function say(v) { log.push(String(v)); }

// 1. eval-introduced var shadows a global mid-loop.
function shadowMidLoop() {
  var seen = [];
  var f = eval("(function () { return typeof Object + ':' + (typeof Object === 'function' ? Object.name : Object); })");
  for (var i = 0; i < 6; i++) {
    if (i === 3) eval("var Object = 'shadowed';");
    seen.push(f());
  }
  return seen.join(' ');
}
say(shadowMidLoop());

// 2. with scope entered between runs of the same site.
function withScope() {
  var g = eval("(function () { return typeof Array === 'function' ? Array.name : String(Array); })");
  var seen = [];
  for (var i = 0; i < 4; i++) seen.push(g());
  with ({ Array: 'obj' }) { seen.push(eval("(function(){ return String(Array); })")()); }
  for (var i = 0; i < 3; i++) seen.push(g());
  return seen.join(' ');
}
say(withScope());

// 3. delete a global property read from eval-made closures.
globalThis.victim = 1;
function deleteGlobal() {
  var h = eval("(function () { return typeof victim === 'undefined' ? 'gone' : victim; })");
  var seen = [];
  for (var i = 0; i < 4; i++) seen.push(h());
  delete globalThis.victim;
  for (var i = 0; i < 3; i++) seen.push(h());
  globalThis.victim = 'back';
  for (var i = 0; i < 2; i++) seen.push(h());
  return seen.join(' ');
}
say(deleteGlobal());

// 4. global getter defined after the first reads; redefinition.
globalThis.gv = 1;
function getterGlobal() {
  var k = eval("(function () { return gv; })");
  var seen = [];
  for (var i = 0; i < 3; i++) seen.push(k());
  var n = 0;
  Object.defineProperty(globalThis, 'gv', { configurable: true, get() { return 'getter' + (++n); } });
  for (var i = 0; i < 3; i++) seen.push(k());
  Object.defineProperty(globalThis, 'gv', { configurable: true, value: 'data', writable: true });
  for (var i = 0; i < 2; i++) seen.push(k());
  return seen.join(' ');
}
say(getterGlobal());

// 6. typeof of undeclared names, and strict vs sloppy assignment to them.
function typeofAndAssign() {
  var u = eval("(function () { return typeof neverDeclared1; })");
  var sloppy = eval("(function () { implicitGlobalA = 5; return typeof implicitGlobalA; })");
  var strict = eval("(function () { 'use strict'; try { implicitGlobalB = 5; return 'assigned'; } catch (e) { return e.constructor.name; } })");
  var seen = [];
  for (var i = 0; i < 3; i++) seen.push(u(), sloppy(), strict());
  seen.push(typeof implicitGlobalA, typeof implicitGlobalB);
  return seen.join(' ');
}
say(typeofAndAssign());

// 7. names through the global object's prototype chain.
Object.prototype.protoName = 'viaProto';
function viaPrototype() {
  var p = eval("(function () { return protoName; })");
  var seen = [];
  for (var i = 0; i < 3; i++) seen.push(p());
  Object.prototype.protoName = 'changed';
  for (var i = 0; i < 2; i++) seen.push(p());
  delete Object.prototype.protoName;
  try { p(); seen.push('no-throw'); } catch (e) { seen.push(e.constructor.name); }
  return seen.join(' ');
}
say(viaPrototype());

// 8. eval-declared var in an enclosing function after the closure ran (function declaration form).
function laterDeclaration() {
  var q = eval("(function () { return typeof Math + ':' + (typeof Math === 'object' ? 'builtin' : Math); })");
  var seen = [];
  for (var i = 0; i < 3; i++) seen.push(q());
  eval("function Math() {}");
  for (var i = 0; i < 3; i++) seen.push(q());
  return seen.join(' ');
}
say(laterDeclaration());

// 9. same eval text from two callers, one of which declares the name.
var text = "(function () { return typeof Date === 'function' ? 'global' : typeof Date; })";
function callerA() { return eval(text); }
function callerB() { eval("var Date = 1;"); return eval(text); }
var a = callerA(), b = callerB();
var seen9 = [];
for (var i = 0; i < 3; i++) seen9.push(a(), b());
say(seen9.join(' '));

// 10. caller-scope variable shadows, closures created in a loop.
function loopClosures() {
  var fs = [];
  for (var i = 0; i < 3; i++) {
    var Number = i === 1 ? 'shadow' : undefined;
    fs.push(eval("(function () { return typeof Number; })"));
  }
  return fs.map(function (f) { return f(); }).join(' ');
}
say(loopClosures());

// 11. calls through names (LoadNameCall) and receivers.
function callNames() {
  var c = eval("(function () { return parseInt('12') + Math.max(1, 2); })");
  var seen = [];
  for (var i = 0; i < 4; i++) seen.push(c());
  eval("var parseInt = function () { return 100; };");
  for (var i = 0; i < 2; i++) seen.push(c());
  return seen.join(' ');
}
say(callNames());

print(log.join('\n'));
"####;

#[test]
fn eval_code_global_reads_follow_every_scope_change() {
    let expected: Vec<&str> = vec![
        r####"function:Object function:Object function:Object string:shadowed string:shadowed string:shadowed"####,
        r####"Array Array Array Array obj Array Array Array"####,
        r####"1 1 1 1 gone gone gone back back"####,
        r####"1 1 1 getter1 getter2 getter3 data data"####,
        r####"undefined number ReferenceError undefined number ReferenceError undefined number ReferenceError number undefined"####,
        r####"viaProto viaProto viaProto changed changed ReferenceError"####,
        r####"object:builtin object:builtin object:builtin function:function Math() {} function:function Math() {} function:function Math() {}"####,
        r####"global number global number global number"####,
        r####"undefined undefined undefined"####,
        r####"14 14 14 14 102 102"####,
    ];
    assert_eq!(output(SCOPE_CHANGES), expected);
}

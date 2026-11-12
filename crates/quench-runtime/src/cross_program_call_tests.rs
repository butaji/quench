//! Calls between separately loaded programs (each indirect eval, `Function` and first-called
//! body is its own program): the lane activates the callee in its program and returns to the
//! caller's, whatever changed between the calls.

use super::subset_tests::output;

const CROSS_PROGRAM_CALLS: &str = r####"
var out = [];
function say() { out.push(Array.prototype.join.call(arguments, ' ')); }

(0, eval)("function addOne(x) { return x + 1; } var counter = { n: 0, bump() { return ++this.n; } };");
(0, eval)("function twice(f, x) { return f(f(x)); } var kinds = { A: class A { constructor(v) { this.v = v; } get doubled() { return this.v * 2; } set doubled(x) { this.v = x / 2; } static make(v) { return new A(v); } } };");
(0, eval)("var sub = function (a, b) { return a - b; }; var thrower = function (m) { throw new RangeError(m); }; var rec = function rec(n, f) { return n === 0 ? 0 : 1 + f(n - 1, rec); };");

var acc = 0;
for (var i = 0; i < 50; i++) {
  acc = addOne(acc);
  if (i === 20) (0, eval)("addOne = function (x) { return x + 10; };");
  if (i === 30) addOne = function (x) { return x + 100; };
}
say('redefined', acc);

var shared = function (x) { return x * 3; };
var fromEval = (0, eval)("(function (n) { var s = 0; for (var i = 0; i < n; i++) s += shared(i); return s; })");
var fromFunction = new Function("n", "var s = 0; for (var i = 0; i < n; i++) s += shared(i); return s;");
var s = 0;
for (var i = 0; i < 20; i++) s += shared(i) + fromEval(5) + fromFunction(5);
say('shared', s);

say('callbacks', twice(addOne, 1), twice(function (x) { return x * 2; }, 3), [1, 2, 3].map(addOne).join(), rec(5, function (n, r) { return r(n, function (k, q) { return q(k, function (a) { return a; }); }); }));

var caught = [];
for (var i = 0; i < 5; i++) {
  try { thrower('boom' + i); } catch (e) { caught.push(e.constructor.name + ':' + e.message + ':' + /thrower/.test(e.stack)); }
}
say('thrown', caught.join('|'));

var k = kinds.A.make(4), g = 0;
for (var i = 0; i < 30; i++) { k.doubled = i; g += k.doubled; }
say('accessors', g, new kinds.A(7).doubled, k instanceof kinds.A);

var px = new Proxy(sub, { apply: function (t, th, a) { return t.apply(null, a) + 1000; } });
var bound = sub.bind(null, 10), p = 0;
for (var i = 0; i < 30; i++) p += px(i, 1) + bound(i);
say('proxy+bound', p);

var c = 0;
for (var i = 0; i < 30; i++) c += counter.bump();
say('method', c, counter.n, counter.bump.call({ n: 5 }));

var ping = function (n) { return n === 0 ? 'done' : pong(n - 1); };
(0, eval)("function pong(n) { return n === 0 ? 'done' : ping(n - 1); }");
say('recursion', ping(1000));

var deepA = function (n) { return deepB(n + 1); };
(0, eval)("function deepB(n) { return deepA(n + 1); }");
try { deepA(0); say('no overflow'); } catch (e) { say('overflow', e.constructor.name); }

(0, eval)("function Ctor(a) { if (!new.target) return 'call'; this.a = a; }");
say('construct', new Ctor(3).a, Ctor(1), Reflect.construct(Ctor, [9]).a);

(0, eval)("function varargs() { return arguments.length + ':' + Array.prototype.join.call(arguments, ','); } function rest(a, ...r) { return a + ':' + r.length; } function dflt(a, b = a + 1) { return a + b; }");
say('args', varargs(1, 2, 3), rest(1, 2, 3, 4), dflt(1), dflt(1, 5), varargs());
out.forEach(function (line) { print(line); });
"####;

#[test]
fn calls_across_programs_match_calls_within_one() {
    let expected: Vec<&str> = vec![
        r####"redefined 2021"####,
        r####"shared 1770"####,
        r####"callbacks 201 12 101,102,103 5"####,
        r####"thrown RangeError:boom0:true|RangeError:boom1:true|RangeError:boom2:true|RangeError:boom3:true|RangeError:boom4:true"####,
        r####"accessors 435 14 true"####,
        r####"proxy+bound 30270"####,
        r####"method 465 30 6"####,
        r####"recursion done"####,
        r####"overflow RangeError"####,
        r####"construct 3 call 9"####,
        r####"args 3:1,2,3 1:3 3 6 0:"####,
    ];
    assert_eq!(output(CROSS_PROGRAM_CALLS), expected);
}

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

const CROSS_PROGRAM_CONSTRUCTS: &str = r####"
var out = [];
function say() { out.push(Array.prototype.join.call(arguments, ' ')); }

(0, eval)("var Base = class Base { constructor(x) { this.x = x; this.nt = new.target.name; } area() { return this.x * 2; } static of(x) { return new this(x); } };");
(0, eval)("var Mid = class Mid extends Base { constructor(x, y) { super(x); this.y = y; } sum() { return this.x + this.y; } };");
(0, eval)("function RetObj(a) { this.a = a; return { replaced: a * 2 }; } function RetPrim(a) { this.a = a; return 5; } function Throws(a) { if (a > 2) throw new Error('bad' + a); this.a = a; }");
var Leaf = new Function("Mid", "return class Leaf extends Mid { constructor() { super(1, 2); this.z = this.sum(); } };")(Mid);

var total = 0;
for (var i = 0; i < 40; i++) {
  var b = new Base(i), m = new Mid(i, 1), l = new Leaf();
  total += b.area() + m.sum() + l.z + (Base.of(i) instanceof Base ? 1 : 0);
}
say('classes', total, new Leaf().nt, new Mid(1, 2).nt, Reflect.construct(Base, [3], Mid).nt);

var r = [];
for (var i = 0; i < 5; i++) {
  var o = new RetObj(i), p = new RetPrim(i);
  r.push(o.replaced + ':' + o.a + ':' + p.a + ':' + (p instanceof RetPrim));
  try { new Throws(i); r.push('ok'); } catch (e) { r.push(e.message); }
}
say('returns', r.join(' '));

class Local extends Base { constructor() { super(7); this.local = true; } }
var n = 0;
for (var i = 0; i < 20; i++) { var q = new Local(); n += q.x + (q.local ? 1 : 0); }
say('local-derived', n, new Local().nt);
out.forEach(function (line) { print(line); });
"####;

#[test]
fn constructs_across_programs_yield_their_receivers_and_replacements() {
    let expected: Vec<&str> = vec![
        r####"classes 2540 Leaf Mid Mid"####,
        r####"returns 0:undefined:0:true ok 2:undefined:1:true ok 4:undefined:2:true ok 6:undefined:3:true bad3 8:undefined:4:true bad4"####,
        r####"local-derived 160 Local"####,
    ];
    assert_eq!(output(CROSS_PROGRAM_CONSTRUCTS), expected);
}

const CROSS_PROGRAM_RETURNING_CALLS: &str = r####"
var out = [];
function say() { out.push(Array.prototype.join.call(arguments, ' ')); }

// Returning calls (`return f(x)`) through chains that cross programs and stay within one.
(0, eval)("function c3(x) { return x + 3; } function c3b(x) { return c3(x) * 2; }");
(0, eval)("function b2(x) { return c3(x + 2); } function b2b(x) { return c3b(x + 2); }");
(0, eval)("function a1(x) { return b2(x + 1); } function a1b(x) { return b2b(x + 1) + 1; }");
var local = function (x) { return a1(x) + a1b(x); };
var tail = function (x) { return local(x); };
var nested = function (x) { return tail(x) + tail(x + 1); };

var t = 0;
for (var i = 0; i < 60; i++) t += nested(i) + a1(i) + (function (v) { return b2b(v); })(i);
say('chains', t);

(0, eval)("function thrower(x) { if (x === 3) throw new Error('t3'); return x; } function viaTail(x) { return thrower(x); } function viaTry(x) { try { return viaTail(x); } finally { fin++; } }");
var fin = 0, results = [];
for (var i = 0; i < 6; i++) {
  try { results.push(viaTry(i)); } catch (e) { results.push(e.message); }
}
say('exceptions', results.join(','), fin);

(0, eval)("function* g1(n) { for (var i = 0; i < n; i++) yield tailAdd(i); } function tailAdd(x) { return addTwo(x); } function addTwo(x) { return x + 2; }");
var gs = [];
for (var v of g1(4)) gs.push(v);
say('generator', gs.join(','));

(0, eval)("async function af(x) { return await tailAsync(x); } function tailAsync(x) { return Promise.resolve(x + 1); }");
var asyncOut = [];
af(1).then(function (v) { asyncOut.push(v); return af(v); }).then(function (v) {
  asyncOut.push(v);
  say('async', asyncOut.join(','));
  out.forEach(function (line) { print(line); });
});

(0, eval)("function deepTail(n) { return n === 0 ? 'bottom' : deepTail2(n - 1); } function deepTail2(n) { return deepTail(n); }");
say('deep', deepTail(2000));

(0, eval)("function sorter(a, b) { return cmp2(a, b); } function cmp2(a, b) { return a - b; }");
say('callbacks', [5, 3, 9, 1].sort(sorter).join(','), [1, 2, 3].map(function (x) { return tailAdd(x); }).join(','));
"####;

#[test]
fn returning_calls_across_programs_complete_in_the_programs_they_began_in() {
    let expected: Vec<&str> = vec![
        r####"chains 19350"####,
        r####"exceptions 0,1,2,t3,4,5 6"####,
        r####"generator 2,3,4,5"####,
        r####"deep bottom"####,
        r####"callbacks 1,3,5,9 3,4,5"####,
        r####"async 2,3"####,
    ];
    assert_eq!(output(CROSS_PROGRAM_RETURNING_CALLS), expected);
}

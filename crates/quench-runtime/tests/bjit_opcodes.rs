//! Per-opcode BaseJIT differential: each family of opcodes runs in a hot loop under `--b-jit`
//! (loop regions built from stencils, every instruction lowered by a row, a helper step or a
//! leave) and in the interpreter, and the two must end identically. The interpreter is the
//! semantic source, so a difference is a b-jit bug. The families are the opcode groups of the
//! bytecode: numeric, relational and branch rows, element and field access, iteration, spread,
//! classes and private names, closures and calls, exceptions (caught in the function, escaping it),
//! `typeof`, `instanceof` and `in`, block scopes, control flow, generators and
//! late type flips that make compiled code leave through its guards.

use quench_runtime::{Engine, ExecutionMode, JitMode, Runtime, SystemHost};

/// Iterations of each family's loop, and runs of the function that holds it: past the tier
/// thresholds of a loop region.
const HOT: u32 = 2_000;
const REPEATS: u32 = 6;

/// Callees, classes and generators the loop bodies use.
const PRELUDE: &str = "var captured = 3;
function mk(n) { return function () { return n + captured; }; }
function add(a, b) { return a + b; }
function C() { this.v = 1; }
C.prototype.get = function () { return this.v; };
class K { x = 1; #p = 2; static s = 3; get y() { return this.#p; } has(o) { return #p in o; } }
function* gen(n) { for (var j = 0; j < n; j++) yield j; }
var kk = new K();
";

/// (opcode family, loop body over `i` with the locals `acc`, `s`, `t`, `o` and `arr`).
const FAMILIES: &[(&str, &str)] = &[
    ("load-const-move-arithmetic", "acc = (acc + i * 3 - 7) | 0; t = acc;"),
    ("bitwise-shift", "acc = ((acc ^ i) & 0xffff) | (i << 3) | (i >>> 2) | (i >> 1);"),
    ("division-modulo-exponent", "acc += (i % 7) + (i / 4) + (i % 5) ** 2;"),
    ("relational-jumps", "if (i % 3 === 0) acc += 2; else if (i < 100) acc -= 1; else if (i >= 1000) acc ^= 1;"),
    ("equality", "acc += (i === 5) + (i !== 6) + (i == '7') + (i != 8) + (null == undefined);"),
    ("increment-decrement", "acc++; --acc; t = i; t++; t--; acc += i & 1 ? 1 : -1;"),
    ("unary", "acc += -i + ~i + +true + (!i ? 1 : 0);"),
    ("truthiness-jumps", "if (acc) acc += 1; if (!t) acc ^= 3; else acc -= 1; t = o.p ? i : 0; if (i % 2 && s) acc += 5;"),
    ("number-negation-not", "acc = (acc + -i + (!acc ? 7 : 0) + -(i / 2)) | 0; t = !i;"),
    ("typeof-void-comma", "s = typeof i + typeof o + typeof s + (void i) + (i, 1);"),
    ("string-concat-compare", "s = (s + i).slice(-12); acc += s < '5' ? 1 : 2;"),
    ("bigint", "t = 5n; t += BigInt(i & 7); t *= 3n; acc += Number(t);"),
    ("array-index", "arr[i & 3] = (arr[(i + 1) & 3] + i) | 0; acc += arr[i & 3];"),
    ("array-append-length", "arr.push(i); if (arr.length > 8) arr.length = 4; acc += arr.length;"),
    ("field-get-set", "o.p = (o.p + i) | 0; o.q = o.p ^ o.q; acc += o.q;"),
    ("field-shape-change", "if (i % 50 === 0) { o.r = i; delete o.r; } acc += o.p;"),
    ("computed-key", "o['k' + (i & 3)] = i; acc += o.k1 === undefined ? 0 : o.k1;"),
    ("for-in", "for (var key in o) acc += key.length;"),
    ("for-in-exhausted-continues-after-loop", "for (var key in o) acc += key.length; acc += 1; for (var k2 in arr) acc += 2;"),
    ("for-in-empty-and-nested", "for (var a in {}) acc += 100; for (var key in o) { for (var b in arr) acc += 1; }"),
    ("for-in-in-callee", "acc += (function (x) { var n = 0; for (var q in x) n += 1; return n; })(o);"),
    ("computed-key-define", "t = {['k' + (i & 1)]: i, [i & 3]: 1}; acc += Object.keys(t).length;"),
    ("class-fields-define", "t = new (class { a = i; ['b' + (i & 1)] = 2; #c = 3; get c() { return this.#c; } })(); acc += t.a + t.c;"),
    ("iterator-close-on-break", "for (var v of gen(5)) { if (v === 2) break; acc += v; }"),
    ("array-literal-spread-holes", "t = [1, ...arr, , i]; acc += t.length;"),
    ("object-rest-spread-keys", "var {p, ...rest} = o; t = {...rest, [s.length]: i}; acc += Object.keys(t).length;"),
    ("property-key-string-int-symbol-object", "var key = i % 4 === 0 ? i : i % 4 === 1 ? 'k' + (i & 3) : i % 4 === 2 ? Symbol.for('s') : { toString() { return 'ts'; } }; t = {[key]: i}; acc += Reflect.ownKeys(t).length;"),
    ("property-key-flips-from-string-to-int", "var key = i < 1500 ? 'k' + (i & 3) : i; t = {[key]: i}; acc += Object.keys(t).length + (typeof Object.keys(t)[0] === 'string' ? 1 : 0);"),
    ("iterator-result-primitive-and-function", "try { for (var v of { [Symbol.iterator]() { return { next() { return i % 3 === 0 ? 1 : i % 3 === 1 ? function () {} : { done: true }; } }; } }) acc += 1; } catch (e) { acc += e instanceof TypeError ? 2 : 3; }"),
    ("iterator-result-record-then-done", "var n = 0; for (var v of { [Symbol.iterator]() { return { next() { return n++ < 3 ? { value: n, done: false } : { done: true }; } }; } }) acc += v;"),
    ("define-field-shape-change", "class B { constructor(n) { if (n) this.pre = 1; } } class D extends B { a = i; b = 2; } t = new D(i % 50 === 0); acc += t.a + t.b + ('pre' in t ? 1 : 0);"),
    ("append-array-non-dense-spread", "var sp = i % 40 === 0 ? [1, , 3] : i % 40 === 1 ? 'ab' : arr; t = [0, ...sp, 9]; acc += t.length;"),
    ("for-in-mutated-object", "var m = {a: 1, b: 2, c: 3}; for (var key in m) { if (key === 'a') { delete m.b; m.d = 4; } acc += key.length; }"),
    ("for-of-break", "for (var v of arr) { if (v > 2) break; acc += v; }"),
    ("array-spread", "t = [...arr, i]; acc += t.length;"),
    ("object-spread", "t = {...o, z: i}; acc += t.z + t.p;"),
    ("destructuring", "var [x, y] = arr; var {p, q} = o; acc += x + y + p + q;"),
    ("closure-capture", "acc += mk(i & 3)();"),
    ("call-arities", "acc += add(i, 1) + add(i) | 0;"),
    ("construct-method", "t = new C(); acc += t.get();"),
    ("class-fields-private-names", "t = new K(); acc += t.x + t.y + (t.has(t) ? 1 : 0) + K.s + kk.y;"),
    ("getter-setter-literal", "t = { get g() { return i; }, set g(v) { acc += v; } }; t.g = 1; acc += t.g;"),
    ("function-names", "t = function () {}; var u = () => 1; s = t.name + u.name;"),
    ("template-literal", "s = `${i}:${acc}`.slice(0, 6);"),
    ("try-catch-finally", "try { if (i % 7 === 0) throw i; acc += 1; } catch (e) { acc += e; } finally { acc += 1; }"),
    ("throw-type-error", "try { null.x; } catch (e) { s = e.constructor.name; } acc += 1;"),
    ("call-every-iteration", "acc = add(acc, i) | 0;"),
    ("throw-every-iteration", "try { throw i; } catch (e) { acc += e; }"),
    ("labeled-loops", "outer: for (var a = 0; a < 3; a++) { for (var b = 0; b < 3; b++) { if (b === 2) continue outer; if (a === 2) break outer; acc += a + b; } }"),
    ("switch", "switch (i & 3) { case 0: acc += 1; break; case 1: acc += 2; case 2: acc += 3; break; default: acc += 4; }"),
    ("generator", "for (var g of gen(3)) acc += g;"),
    ("in-instanceof", "t = new C(); acc += ('p' in o) + (t instanceof C ? 1 : 0) + (arr instanceof Array ? 1 : 0);"),
    ("throw-caught-with-and-without-binding", "try { if (i & 1) throw i; acc += 1; } catch (e) { acc += e; } try { throw 1; } catch { acc += 2; }"),
    ("throw-nested-catch-rethrow", "try { try { throw i; } catch (e) { acc += e; throw e + 1; } } catch (f) { acc += f; }"),
    ("throw-escaping-a-callee", "try { (function (x) { if (x & 1) throw { v: x }; return 0; })(i); } catch (e) { acc += e.v; }"),
    ("throw-non-error-values", "try { throw i & 1 ? 'k' : null; } catch (e) { s = String(e); }"),
    ("typeof-variants", "s = typeof i + typeof s + typeof o + typeof null + typeof undefined + typeof arr + typeof C + typeof Symbol() + typeof 1n + typeof gen + typeof notDefinedAnywhere;"),
    ("typeof-compared", "acc += (typeof t === 'number' ? 1 : 0) + (typeof o === 'object' ? 2 : 0) + (typeof s !== 'string' ? 4 : 0);"),
    ("instanceof-variants", "t = new C(); var h = { [Symbol.hasInstance](v) { return v > 3; } }; acc += (t instanceof C) + (o instanceof C) + (C instanceof Function) + (arr instanceof Object) + (i instanceof h);"),
    ("in-operator-variants", "var sy = Symbol.iterator; acc += ('p' in o) + (1 in arr) + ('length' in arr) + ('zz' in o) + (i in arr) + (sy in arr);"),
    ("in-operator-throws", "try { acc += 'p' in i; } catch (e) { acc += e instanceof TypeError ? 1 : 0; }"),
    ("block-scopes-with-closures", "var fs = []; for (let j = 0; j < 3; j++) { let c = j * i; fs.push(function () { return c + j; }); } acc += fs[0]() + fs[1]() + fs[2]();"),
    ("block-scope-shadow-and-tdz", "{ let a = i; { let a = 2; acc += a; } acc += a; } try { acc += zz; let zz = 1; } catch (e) { acc += 1; }"),
    ("class-construction-and-super", "class E extends C { constructor() { super(); this.w = 5; } m() { return super.get() + this.w; } } t = new E(); acc += t.w + t.get() + t.m();"),
    ("class-heritage-check", "try { class Bad extends 5 {} } catch (e) { acc += 1; } class Ok extends (i & 1 ? C : Object) {} acc += new Ok() instanceof Object ? 1 : 0;"),
    ("optional-nullish", "t = null; acc += (t?.x ?? 1) + (o?.p ?? 0);"),
    ("delete", "t = {a: 1, b: 2}; delete t.a; acc += Object.keys(t).length;"),
    ("arguments-this", "acc += (function () { return arguments.length + (this === undefined ? 0 : 1); })(i, i);"),
    ("regexp-literal", "acc += /a(b)?/.test('ab' + (i & 1)) ? 1 : 0;"),
    ("native-calls", "acc += Math.floor(i / 3) + Math.max(i, 5) + Math.abs(-i) | 0;"),
    ("string-methods", "s = 'abc'.charAt(i % 3) + 'x'.repeat(i & 1);"),
    ("object-and-array-literals", "t = [i, {a: i}, [i]]; acc += t[1].a + t[2][0];"),
    ("array-holes", "t = [1, , 3]; acc += t.length + (1 in t ? 1 : 0);"),
    ("late-type-flip", "t = i < 1500 ? i : 'k' + i; acc += typeof t === 'number' ? 1 : 2;"),
    ("int32-overflow-flip", "t = i < 1600 ? 2147483000 + i : i; acc = (acc + t) | 0; acc += t * 2 > 4294967296 ? 1 : 0;"),
    ("negative-zero", "t = i % 2 ? -0 : 0; acc += (1 / t) < 0 ? 1 : 2;"),
    ("backward-conditional-loop-over-non-numbers", "var w = 0, z = 'a'; do { w++; z = z + 'b'; } while (z.length < 3 && w < 5); acc += w + z.length;"),
    ("backward-conditional-loop-with-in-and-instanceof", "var u = 0; do { u++; } while (u < 4 && !(u in arr) && !(t instanceof C)); acc += u;"),
    ("backward-conditional-nested-with-break", "for (var a = 0; a < 3; a++) { var b = 0; while (b < 4 && o.p) { b++; if (b === 3) break; } acc += a + b; }"),
    ("returning-arithmetic-field-unary-relational-typeof", "var f = function (a, b) { return a + b; }, g = function (v) { return v.p; }, h = function (a) { return -a; }, k = function (a, b) { return a < b; }, m = function (v) { return typeof v; }; acc += f(i, 1) + g(o) + h(i & 3) + (k(i, 9) ? 1 : 0) + m(i).length;"),
    ("returning-calls-chain", "var leaf = function (x) { return x + 1; }, mid = function (x) { return leaf(x); }; acc += mid(i) | 0;"),
    ("returning-misses", "var f2 = function (a, b) { return a + b; }; acc += f2(i < 1500 ? i : 'k', 1).length | 0;"),
    ("call-method-this-method", "var ob = { b: 2, m(x) { return x + this.b; }, n() { return this.m(1); } }; acc += ob.m(i) + ob.n();"),
    ("call-spread-array-and-forwarded", "var sum3 = function () { var r = 0; for (var q = 0; q < arguments.length; q++) r += arguments[q]; return r; }; acc += sum3(...arr) + sum3(i, ...arr) + sum3.apply(null, arr) + sum3.call(null, i);"),
    ("construct-returning-and-super", "class P2 { constructor(v) { this.v = v; } } class Q2 extends P2 { constructor(v) { super(v + 1); } } var mk2 = function (v) { return new Q2(v); }; acc += mk2(i & 3).v + new P2(i & 1).v;"),
    ("name-call-of-a-global", "acc += Math.max(i, 5) + parseInt('12', 10) + add(i, 1);"),
    ("async-function", "(async function () { await 0; acc += 1; })();"),
];

/// A program that runs `body` hot and ends by throwing a digest of what it computed, so the
/// outcome of a run is the whole of its observable state.
fn program(body: &str) -> String {
    format!(
        "{PRELUDE}function run(n) {{ var acc = 0, s = '', t = 0, o = {{p: 1, q: 2}}, arr = [1, 2, 3, 4];
  for (var i = 0; i < n; i++) {{ {body} }}
  return acc + '|' + s + '|' + String(t) + '|' + JSON.stringify(o) + '|' + JSON.stringify(arr); }}
var out = ''; for (var k = 0; k < {REPEATS}; k++) out += run({HOT}) + ';';
throw out;
"
    )
}

fn run(source: &str, mode: ExecutionMode) -> String {
    let outcome = Engine::specialize(source, "bjit-opcodes.js")
        .map_err(|errors| format!("{errors:?}"))
        .and_then(|program| {
            let mut runtime = Runtime::new(SystemHost);
            runtime.set_execution_mode(mode);
            runtime.execute(&program).map_err(|error| error.to_string())
        });
    format!("{outcome:?}")
}

#[test]
fn every_opcode_family_ends_in_b_jit_as_the_interpreter_does() {
    JitMode::BaseOnly.set_process_mode();
    let mut divergent = Vec::new();
    for (family, body) in FAMILIES {
        let source = program(body);
        let interpreted = run(&source, ExecutionMode::Interpreted);
        assert!(interpreted.contains('|'), "{family} did not run to its digest: {interpreted}");
        let compiled = run(&source, ExecutionMode::Compiled);
        if interpreted != compiled {
            divergent.push(format!("{family}:\n  interpreter {interpreted}\n  b-jit {compiled}"));
        }
    }
    assert!(divergent.is_empty(), "{} of {} families diverge\n{}", divergent.len(), FAMILIES.len(), divergent.join("\n"));
}

//! Lane semantics of own symbol proofs on accessor-bearing shapes, strict equality by kind, and
//! class field defines on receivers with extras.
use super::Vm;
use crate::Engine;
use crate::test_host::TestHost;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn class_fields_define_on_receivers_with_and_without_extras() {
    // A field's cached define skips receivers whose extras hold boxed-primitive, stack or error
    // data; plain receivers, subclasses of builtins and return-overridden objects all define
    // their fields (never set them), in order.
    let source = r#"
let setterCalls = 0;
class Base { set a(v) { setterCalls++; } }
class F extends Base { a = 1; b = 2; c = this.a + this.b; d = 4; }
class E1 extends Error { code = 'E1'; extra = this.message.length; x = 1; y = 2; }
class N1 extends Number { tag = 't'; a = 1; b = 2; c = 3; }
class Ret { constructor(o) { return o; } }
class R1 extends Ret { a = 1; b = 2; c = 3; }
const out = [];
for (let i = 0; i < 4; i++) {
  const f = new F(), e = new E1('boom'), n = new N1(5), r = {}; new R1(r);
  if (i === 3) out.push(JSON.stringify(f), setterCalls, e.code, e.extra, typeof e.stack, n.tag, n.c, +n, JSON.stringify(r));
}
print(out.join(','));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "class-field-extras.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"{"a":1,"b":2,"c":3,"d":4},0,E1,4,string,t,3,5,{"a":1,"b":2,"c":3}"#]);
}

#[test]
fn strict_equality_decides_symbols_and_distinct_heap_values_by_identity_or_contents() {
    // Symbols, objects and functions equal only themselves; strings and BigInts of equal
    // contents are equal, whichever form (jump, value, switch case) the comparison takes.
    let source = r#"
const s1 = Symbol('x'), s2 = Symbol('x'), r1 = Symbol.for('r'), r2 = Symbol.for('r');
const boxed = Object(s1), text1 = 'ab' + Math.floor(1.5), big = 10n ** 30n, big2 = 10n ** 30n;
const vals = [s1, s2, r1, r2, Symbol.iterator, boxed, Object(s1), big, big2, 5n, text1, 'ab1', {}, {}, () => 1, [], [], 1, NaN, null, undefined];
function jump(a, b) { if (a === b) return 1; return 0; }
function value(a, b) { return a !== b; }
function sw(a, b) { switch (a) { case b: return 1; default: return 0; } }
const out = [];
for (let round = 0; round < 3; round++) {
  out.length = 0;
  for (const a of vals) for (const b of vals) out.push(jump(a, b) + (value(a, b) ? 0 : 2) + sw(a, b) * 4);
}
print(out.map(String).join(''));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "strict-equality-kinds.js").unwrap()).unwrap();
    // Equal pairs: the diagonal (but NaN), the registered symbol, the BigInt and string pairs.
    let row = output.borrow()[0].clone();
    let width = 21;
    let cell = |a: usize, b: usize| row.as_bytes()[a * width + b] - b'0';
    assert_eq!(cell(0, 0), 7);
    assert_eq!(cell(0, 1), 0);
    assert_eq!(cell(2, 3), 7);
    assert_eq!(cell(7, 8), 7);
    assert_eq!(cell(10, 11), 7);
    assert_eq!(cell(5, 6), 0);
    assert_eq!(cell(12, 13), 0);
    assert_eq!(cell(18, 18), 0);
    assert_eq!(cell(19, 19), 7);
}

#[test]
fn symbol_reads_on_accessor_bearing_shapes_follow_the_slot_kind() {
    // An own data symbol slot of a shape that also holds accessors is proved for the shape;
    // the accessor slots, a redefinition of the data slot as an accessor and a second
    // object of another shape keep the ordinary [[Get]] answers.
    let source = r#"
const k = Symbol('k'), g = Symbol('g');
const out = [];
function read(o, key) { let r; for (let i = 0; i < 4; i++) r = o[key]; return r; }
function make(n) { return { get [g]() { return 'get' + n; }, [k]: n }; }
const a = make(1), b = make(2);
out.push(read(a, k), read(b, k), read(a, g), read(b, g), read(a, k));
Object.defineProperty(a, k, { get() { return 'now-getter'; }, configurable: true });
out.push(read(a, k), read(b, k));
delete b[k]; out.push(String(read(b, k)), read(make(3), k));
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "symbol-accessor-shapes.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"[1,2,"get1","get2",1,"now-getter",2,"undefined",3]"#]);
}

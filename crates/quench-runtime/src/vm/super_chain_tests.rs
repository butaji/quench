use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// Shared by every source: `log` renders each object with its constructor and key order, and
/// `rep` constructs often enough for the lane path and its construct plan to run.
const PRELUDE: &str = r#"
const out = [];
const log = (...a) => out.push(a.map((x) => typeof x === "object" && x ? JSON.stringify(x) + ":" + Object.getPrototypeOf(x).constructor.name + ":" + Object.keys(x).join(",") : String(x)).join(" "));
function rep(f, n = 3000) { let r; for (let i = 0; i < n; i++) r = f(i); return r; }
"#;

/// What `source`, after the prelude, prints.
pub(in crate::vm) fn printed(source: &str, file: &str) -> Vec<String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(&format!("{PRELUDE}{source}"), file).unwrap();
    vm.execute(&program).unwrap();
    output.take()
}

/// A chain of plain constructors (and classes with none) builds one object: missing
/// arguments store `undefined`, extra ones are dropped, constants, swapped and repeated
/// parameters land in the parent's order of stores.
#[test]
fn chain_constructions_pass_missing_extra_and_constant_arguments() {
    let source = r#"
class N {}
class V extends N {}
class E extends V { constructor(o) { super(); this._o = o; } }
class P extends E { constructor(o, b) { super(o); this.base = b; this.lv = null; this.s = "str"; this.n = 1.5; this.t = true; } }
class D extends P { constructor(o, s, f) { super(o, s); this.f = f; } }
log(rep((i) => new D(i, "b", [i])), rep((i) => new D()), rep((i) => new D(1, 2, 3, 4, 5)));
class A2 { constructor(a, b, c) { this.a = a; this.b = b; this.c = c; } }
class B2 extends A2 { constructor(x, y) { super(y, 7, x); this.x = x; } }
class C2 extends B2 { constructor(q) { super(q, q); } }
class D2 extends A2 { constructor(x) { super(x); this.d = undefined; } }
log(rep((i) => new B2(1, i)), rep((i) => new C2("q")), rep((i) => new D2(9)), rep((i) => new D2()));
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "chain-arguments.js"),
        [r#"{"_o":2999,"base":"b","lv":null,"s":"str","n":1.5,"t":true,"f":[2999]}:D:_o,base,lv,s,n,t,f {"lv":null,"s":"str","n":1.5,"t":true}:D:_o,base,lv,s,n,t,f {"_o":1,"base":2,"lv":null,"s":"str","n":1.5,"t":true,"f":3}:D:_o,base,lv,s,n,t,f | {"a":2999,"b":7,"c":1,"x":1}:B2:a,b,c,x {"a":"q","b":7,"c":"q","x":"q"}:C2:a,b,c,x {"a":9}:D2:a,b,c,d {}:D2:a,b,c,d"#]
    );
}

/// A function root's conditional and numeric stores keep their values, and a parent or
/// grandparent replaced by `Object.setPrototypeOf` after warm-up builds through the new chain.
#[test]
fn chain_constructions_follow_function_roots_and_reparenting() {
    let source = r#"
function F3(a, b) { this.a = a ? a : 5; this.b = b + 1; }
class G3 extends F3 { constructor(a, b) { super(a, b); this.g = a; } }
class H3 extends F3 { constructor(a) { super(0, 2); this.h = a; } }
class I3 extends F3 { constructor(a) { super(a, "x"); } }
log(rep((i) => new G3(0, i)), rep((i) => new G3(3, 4)), rep((i) => new H3(1)), rep((i) => new I3(1)));
class A4 { constructor(v) { this.a4 = v; } }
class Z4 { constructor(v) { this.z4 = v; this.extra = 1; } }
class B4 extends A4 { constructor(v) { super(v); this.b4 = v; } }
class C4 extends B4 { constructor(v) { super(v); this.c4 = v; } }
log(rep((i) => new C4(i)));
Object.setPrototypeOf(B4, Z4);
log(rep((i) => new C4(i)), new C4(2) instanceof A4);
Object.setPrototypeOf(C4, Z4);
log(rep((i) => new C4(i)), new C4(2) instanceof B4);
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "function-root-reparenting.js"),
        [r#"{"a":5,"b":3000,"g":0}:G3:a,b,g {"a":3,"b":5,"g":3}:G3:a,b,g {"a":5,"b":3,"h":1}:H3:a,b,h {"a":1,"b":"x1"}:I3:a,b | {"a4":2999,"b4":2999,"c4":2999}:C4:a4,b4,c4 | {"z4":2999,"extra":1,"b4":2999,"c4":2999}:C4:z4,extra,b4,c4 true | {"z4":2999,"extra":1,"c4":2999}:C4:z4,extra,c4 true"#]
    );
}

/// A setter that appears on a prototype takes the store until it is deleted; `Reflect.construct`
/// with another `new.target`, a class without a constructor and one reading `new.target` keep
/// their prototypes and fields.
#[test]
fn chain_constructions_keep_setters_and_new_target() {
    let source = r#"
class A5 { constructor(v) { this.v = v; } }
class B5 extends A5 { constructor(v) { super(v); this.w = v; } }
log(rep((i) => new B5(i)));
let hits = 0;
Object.defineProperty(A5.prototype, "w", { set(x) { hits++; }, get() { return "acc"; }, configurable: true });
log(rep((i) => new B5(i)), hits, new B5(2).w);
delete A5.prototype.w;
log(rep((i) => new B5(i)), hits);
class P6 { constructor(o, b) { this.o = o; this.b = b; } }
class D extends P6 { constructor(o, s, f) { super(o, s); this.f = f; } }
class S6 extends D {}
class T6 extends D { constructor() { super(1, 2, 3); this.t = new.target === T6; } }
function Other() {} Other.prototype = { tag: 1 };
const viaReflect = rep((i) => Reflect.construct(D, [i, 2, 3], Other));
log(rep((i) => new S6(i, 2, 3)), rep((i) => new T6()), viaReflect.tag, viaReflect instanceof D, Object.keys(viaReflect).join());
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "setter-and-new-target.js"),
        [r#"{"v":2999,"w":2999}:B5:v,w | {"v":2999}:B5:v 3000 acc | {"v":2999,"w":2999}:B5:v,w 3001 | {"o":2999,"b":2,"f":3}:S6:o,b,f {"o":1,"b":2,"f":3,"t":true}:T6:o,b,f,t 1 false o,b,f"#]
    );
}

/// Fields, private names, a root returning an object, a throwing root, `this` before `super`,
/// and function, `Map` and `Error` parents keep the general construct semantics.
#[test]
fn chain_constructions_keep_non_plain_constructor_semantics() {
    let source = r#"
class N {}
class A7 { x = 1; constructor(v) { this.v = v; } }
class B7 extends A7 { y = 2; constructor(v) { super(v); this.w = v; } }
class C7 extends B7 { #p = 3; constructor(v) { super(v); this.z = this.#p; } }
log(rep((i) => new C7(i)));
class A8 { constructor(v) { return { over: v }; } }
class B8 extends A8 { constructor(v) { super(v); this.b = 1; } }
class A9 { constructor(v) { if (v > 2998) throw new Error("boom" + v); this.v = v; } }
class B9 extends A9 { constructor(v) { super(v); this.b = v; } }
let err = ""; try { rep((i) => new B9(i)); } catch (e) { err = e.message; }
log(rep((i) => new B8(i)), err);
class B10 extends N { constructor(v) { this.x = v; super(); } }
for (let i = 0; i < 3; i++) try { new B10(i); } catch (e) { log(e.constructor.name); }
function F11(a) { this.a = a; }
class G11 extends F11 { constructor(a) { super(a); this.g = 1; } }
class M11 extends Map { constructor(a) { super(); this.m = a; } }
class E11 extends Error { constructor(m) { super(m); this.code = 7; } }
log(rep((i) => new G11(i)), rep((i) => new M11(i)).m, rep((i) => new E11("msg" + i)).message, new E11("z").code, new E11("z") instanceof E11, new M11(1) instanceof Map);
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "non-plain-roots.js"),
        [r#"{"x":1,"v":2999,"y":2,"w":2999,"z":3}:C7:x,v,y,w,z | {"over":2999,"b":1}:Object:over,b boom2999 | ReferenceError | ReferenceError | ReferenceError | {"a":2999,"g":1}:G11:a,g 2999 msg2999 7 true true"#]
    );
}

/// A field stored at two levels keeps its first position, more than eight fields and permuted
/// parameters store in order, and default, rest and `arguments` parameters keep their values.
#[test]
fn chain_constructions_keep_field_order_across_levels() {
    let source = r#"
class A12 { constructor(a) { this.k = a; this.z = 0; } }
class B12 extends A12 { constructor(a) { super(a); this.k = "over"; this.a = 1; this.b = 2; this.c = 3; this.d = 4; this.e = 5; this.f = 6; this.g = 7; this.h = 8; } }
log(rep((i) => new B12(i)));
class W12 { constructor(a, b, c, d, e) { this.a = a; this.b = b; this.c = c; this.d = d; this.e = e; } }
class X12 extends W12 { constructor(a, b, c, d, e, f) { super(e, d, c, b, a); this.f = f; this.g = a; this.h = b; this.i = c; this.j = 0; } }
log(rep((i) => new X12(i, 1, 2, 3, 4, 5)));
class B13 extends A12 { constructor(a = 3) { super(a); this.q = a; } }
class B14 extends A12 { constructor(...r) { super(r[0]); this.q = r.length; } }
class B15 extends A12 { constructor(a) { super(arguments[1]); this.q = a; } }
log(rep((i) => new B13()), rep((i) => new B14(i, 2)), rep((i) => new B15(1, i)));
class A16 { constructor(a) { this.a = a; } m() { return this.a * 2; } }
class B16 extends A16 { constructor(a) { super(a); this.b = a; } m() { return super.m() + 1; } }
let sum = 0; for (let i = 0; i < 3000; i++) sum += new B16(i).m();
log(sum, rep((i) => new B16(1)).constructor === B16);
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "field-layouts.js"),
        [r#"{"k":"over","z":0,"a":1,"b":2,"c":3,"d":4,"e":5,"f":6,"g":7,"h":8}:B12:k,z,a,b,c,d,e,f,g,h | {"a":4,"b":3,"c":2,"d":1,"e":2999,"f":5,"g":2999,"h":1,"i":2,"j":0}:X12:a,b,c,d,e,f,g,h,i,j | {"k":3,"z":0,"q":3}:B13:k,z,q {"k":2999,"z":0,"q":2}:B14:k,z,q {"k":2999,"z":0,"q":1}:B15:k,z,q | 9000000 true"#]
    );
}

/// `return new C(...)` of a chain completes its activation with the receiver: from a method,
/// an arrow, under a `finally`, and from async and generator bodies.
#[test]
fn chain_constructions_return_from_their_activation() {
    let source = r#"
class N {} class E extends N { constructor(o) { super(); this.o = o; } }
class V { visit(o) { return new E(o); } }
async function a(o) { return new E(o); }
function* g() { for (let i = 0; i < 3; i++) yield new E(i); }
const v = new V(); let s = 0;
for (let i = 0; i < 100000; i++) s += v.visit(i).o;
const f = (o) => new E(o); for (let i = 0; i < 1000; i++) s += f(i).o;
function tail(o) { try { return new E(o); } finally { s++; } }
for (let i = 0; i < 1000; i++) s += tail(i).o;
a(5).then(x => print([s, x.o, x instanceof E, [...g()].map(e => e.o).join()].join(" ")));
"#;
    assert_eq!(printed(source, "chain-returns.js"), ["5000949000 5 true 0,1,2"]);
}

/// A class extending `Array` constructs its array in the lane with its own `prototype`, a
/// length that is no array length raises a RangeError, and another `new.target` names the
/// prototype.
#[test]
fn array_subclass_constructions_keep_prototype_and_new_target() {
    let source = r#"
class M extends Array { constructor(n) { super(n); } }
M.prototype.klass = "m";
log(rep((i) => new M(3)).length, rep((i) => new M(2)).klass, new M(1, 2, 3).length, new M("a")[0], Array.isArray(new M(2)) && new M(2) instanceof M);
try { rep((i) => new M(-1)); } catch (e) { log(e.name); }
function F() {} F.prototype = Object.create(Array.prototype); F.prototype.tag = "F";
log(Reflect.construct(M, [3], F).tag, Reflect.construct(M, [3], F) instanceof M);
print(out.join(" | "));
"#;
    assert_eq!(printed(source, "array-subclass.js"), ["3 m 1 a true | RangeError | F false"]);
}

/// A constructor that spreads its rest list into `super(...)` and stores constants builds the
/// chain's object (arguments passed on whole, the missing ones `undefined`); one that stores
/// the list's own value, another `new.target` and a reparented parent keep their results.
#[test]
fn rest_forwarding_constructors_with_stores_join_the_chain() {
    let source = r#"
class P1 { constructor(a, b) { this.a = a; this.b = b; } }
class RF extends P1 { constructor(...args) { super(...args); this.y = 1; } }
class RF2 extends RF { constructor(...args) { super(...args); this.z = "k"; this.u = null; } }
class RF3 extends P1 { constructor(...args) { super(...args); this.y = args.length; } }
log(rep((i) => new RF(i, 2)), rep((i) => new RF2(i)), rep((i) => new RF3(i, 2, 3)), rep(() => new RF2()));
function NT() {} NT.prototype = { mark: "nt" };
log(Reflect.construct(RF2, [1, 2], NT).mark, Reflect.construct(RF2, [1, 2], NT) instanceof RF2);
class Z9 { constructor(v) { this.z9 = v; } }
Object.setPrototypeOf(RF, Z9);
log(rep((i) => new RF2(i)));
print(out.join(" | "));
"#;
    assert_eq!(
        printed(source, "rest-forwarding.js"),
        [r#"{"a":2999,"b":2,"y":1}:RF:a,b,y {"a":2999,"y":1,"z":"k","u":null}:RF2:a,b,y,z,u {"a":2999,"b":2,"y":3}:RF3:a,b,y {"y":1,"z":"k","u":null}:RF2:a,b,y,z,u | nt false | {"z9":2999,"y":1,"z":"k","u":null}:RF2:z9,y,z,u"#]
    );
}

/// A chain whose classes belong to other programs than the one constructing builds the same
/// objects, before and after a class in it is reparented.
#[test]
fn chain_constructions_cross_programs() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let sources = [
        "globalThis.P = class P { constructor(x) { this.x = x; } }; globalThis.Q = class Q extends P { constructor(x, y) { super(x); this.y = y; } };",
        "globalThis.R = class R { constructor(a) { this.a = a; this.b = a + 1; } };",
        "function go() { let s = 0; for (let i = 0; i < 3000; i++) s += new Q(i, 2).y + new R(i).b + new P(i).x; return s + JSON.stringify(new Q(1, 2)) + Object.keys(new Q(1, 2)); } print(go()); Object.setPrototypeOf(Q, R); print(go());",
        "globalThis.B1 = class B1 { constructor(n) { this.n = n; } };",
        "globalThis.D1 = class D1 extends B1 { constructor() { super('passed-literal'); } };",
        "let t = ''; for (let i = 0; i < 3000; i++) t = new D1().n; print(t);",
    ];
    let first = Engine::specialize(sources[0], "cross-0.js").unwrap();
    vm.execute(&first).unwrap();
    for (index, source) in sources.iter().enumerate().skip(1) {
        vm.eval_script_source(&first, source, &format!("cross-{index}.js")).unwrap();
    }
    assert_eq!(
        output.take(),
        ["9006000{\"x\":1,\"y\":2}x,y", "9006000{\"a\":1,\"b\":2,\"y\":2}a,b,y", "passed-literal"].map(String::from)
    );
}

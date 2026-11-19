//! Derived class constructors pushed in place (`Vm::inline_derived_construct_target`,
//! `CallCompletion::DerivedConstructor`) and `super(...)` pushed in place
//! (`Vm::inline_super_construct_target`): each case constructs often enough for the lane and
//! the general loop to take the in-place path, and prints what Node prints for it.
use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

const PRELUDE: &str = r#"
function rep(f, n = 2000) { let r; for (let i = 0; i < n; i++) r = f(i); return r; }
function thrown(f) { try { f(); return "none"; } catch (e) { return e.constructor.name; } }
"#;

fn printed(source: &str, file: &str) -> Vec<String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(&format!("{PRELUDE}{source}"), file).unwrap();
    vm.execute(&program).unwrap();
    output.take()
}

/// Code before and after `super(...)`, a three-class chain, `this` bound only by `super`,
/// a missing `super`, `this` before `super`, a second `super`, and `super` from an arrow.
#[test]
fn derived_constructors_run_code_around_super_and_keep_this_rules() {
    let source = r#"
class A { constructor(x) { this.a = x; } }
class B extends A { constructor(x, o) { const d = o ? o.d : 1; super(x + d); this.b = this.a * 2; } }
class C extends B { constructor(x) { if (x < 0) x = 0; super(x, { d: 10 }); this.c = this.b + 1; } }
const c = rep((i) => new C(i));
print([JSON.stringify(c), Object.keys(c).join(","), c instanceof C && c instanceof B && c instanceof A, c.constructor.name].join(" "));
class M extends A { constructor(x) { if (x) super(x); } }
print([rep(() => thrown(() => new M(0))), rep(() => new M(1).a)].join(" "));
class T extends A { constructor() { this.z = 1; super(1); } }
class D2 extends A { constructor() { super(1); super(2); } }
print([rep(() => thrown(() => new T())), rep(() => thrown(() => new D2()))].join(" "));
class AR extends A { constructor(x) { const f = () => super(x + 1); f(); this.y = this.a; } }
print(String(rep((i) => new AR(i)).y));
class L0 { constructor(v) { this.v = [v]; } }
class L1 extends L0 { constructor(v) { super(v + 1); this.v.push(1); } }
class L2 extends L1 { constructor(v) { super(v + 1); this.v.push(2); } }
class L3 extends L2 { constructor(v) { super(v + 1); this.v.push(3); } }
class L4 extends L3 { constructor(v) { super(v + 1); this.v.push(4); } }
print(rep((i) => new L4(i)).v.join("-"));
"#;
    assert_eq!(
        printed(source, "derived-construct-this.js"),
        [
            r#"{"a":2009,"b":4018,"c":4019} a,b,c true C"#,
            "ReferenceError 1",
            "ReferenceError ReferenceError",
            "2000",
            "2003-1-2-3-4",
        ]
    );
}

/// The derived return rules: an object overrides, `undefined` yields `this`, a primitive is a
/// TypeError, `undefined` before `super` a ReferenceError, an object without `super` is kept;
/// a parent's returned object becomes the child's `this`.
#[test]
fn derived_constructors_apply_the_return_rules() {
    let source = r#"
class A { constructor(x) { this.a = x; } }
class R extends A { constructor(k) { super(k); if (k % 2) return { odd: k }; } }
class RP extends A { constructor() { super(1); return 5; } }
class RU extends A { constructor() { return undefined; } }
class RO extends A { constructor() { return { o: 1 }; } }
print([JSON.stringify(rep((i) => new R(i))), new R(2) instanceof R, new R(2).a,
  rep(() => thrown(() => new RP())), rep(() => thrown(() => new RU())), JSON.stringify(rep(() => new RO()))].join(" "));
class PO { constructor() { return { po: 1 }; } }
class DPO extends PO { constructor() { super(); this.d = 2; } }
const d = rep(() => new DPO());
print([JSON.stringify(d), d instanceof DPO].join(" "));
"#;
    assert_eq!(
        printed(source, "derived-construct-returns.js"),
        [r#"{"odd":1999} true 2 TypeError ReferenceError {"o":1}"#, r#"{"po":1,"d":2} false"#]
    );
}

/// `new.target` through the chain and through `Reflect.construct`, class fields on both sides
/// of `super`, `extends null`, native parents (`Array`, `Error`) and a proxy parent.
#[test]
fn derived_constructors_keep_new_target_fields_and_native_parents() {
    let source = r#"
class NT { constructor() { this.t = new.target.name; } }
class NT1 extends NT { constructor() { super(); this.u = new.target === NT2; } }
class NT2 extends NT1 { constructor() { super(); } }
const n2 = rep(() => new NT2());
const rc = Reflect.construct(NT1, [], NT2);
print([n2.t, n2.u, Object.getPrototypeOf(n2) === NT2.prototype, rc.t, rc.u, Object.getPrototypeOf(rc) === NT2.prototype].join(" "));
class F0 { f0 = 1; constructor(x) { this.x = x; } }
class F1 extends F0 { f1 = this.x + 1; constructor(x) { super(x * 2); this.y = this.f1 * 10; } }
print(JSON.stringify(rep((i) => new F1(i))));
class Nl extends null { constructor() { super(); } }
class Nl2 extends null { constructor() { return Object.create(Nl2.prototype); } }
print([rep(() => thrown(() => new Nl())), rep(() => new Nl2()) instanceof Nl2].join(" "));
class Arr extends Array { constructor(n) { super(); this.tag = n; for (let i = 0; i < n; i++) this.push(i); } }
const arr = rep((i) => new Arr(i % 5));
class Err extends Error { constructor(m) { super("E:" + m); this.name = "Err"; this.code = m; } }
const er = rep((i) => new Err(i));
print([arr.length, arr.tag, Array.isArray(arr), arr instanceof Arr, JSON.stringify(arr),
  er.message, er.code, er instanceof Error, String(er), typeof er.stack].join(" "));
class A { constructor(x) { this.a = x; } }
const PP = new Proxy(A, {});
class DP extends PP { constructor(x) { super(x); this.p = 1; } }
const dp = rep(() => new DP(3));
print([JSON.stringify(dp), dp instanceof A].join(" "));
"#;
    assert_eq!(
        printed(source, "derived-construct-targets.js"),
        [
            "NT2 true true NT2 true true",
            r#"{"f0":1,"x":3998,"f1":3999,"y":39990}"#,
            "TypeError true",
            "4 4 true true [0,1,2,3] E:1999 1999 true Err: E:1999 string",
            r#"{"a":3,"p":1} true"#,
        ]
    );
}

/// A parent constructor's exception reaches the derived constructor's handler with `this` still
/// uninitialized, so a second `super(...)` binds it; uncaught, it reaches the `new`.
#[test]
fn exceptions_in_parent_constructors_leave_this_uninitialized() {
    let source = r#"
class PX { constructor(x) { if (x === 3) throw new Error("p" + x); this.x = x; } }
class DX extends PX { constructor(x) { super(x); this.s = "ok"; } }
class DY extends PX { constructor(x) { try { super(x); } catch (e) { super(0); this.e = e.message; } } }
const dx = rep((i) => { try { return new DX(i % 4).s; } catch (e) { return e.message; } });
print([dx, rep(() => new DX(1).s), JSON.stringify(rep((i) => new DY(i % 4)))].join(" "));
"#;
    assert_eq!(printed(source, "derived-construct-throws.js"), [r#"p3 ok {"x":0,"e":"p3"}"#]);
}

/// The steps of `super(...)` the lane makes in the frame it runs (`dispatch_fast::derived_frames`):
/// an implicit constructor, whose rest list applies as a dense array, and spread arguments; an
/// instance initializer that throws once `this` is bound, caught in the constructor; a second
/// `super` caught after the parent ran again; `this` read before `super`, caught, then `super`.
#[test]
fn super_steps_run_in_the_frame_the_lane_runs() {
    let source = r#"
class B0 { constructor(a, b) { this.s = a + b; } }
class B1 extends B0 {}
class B2 extends B1 { constructor(a) { super(a, 1); this.t = this.s * 2; } }
class B3 extends B0 { constructor(...xs) { super(...xs); this.n = xs.length; } }
print([JSON.stringify(rep((i) => new B2(i))), rep((i) => new B2(i)) instanceof B1, JSON.stringify(rep((i) => new B3(i, 2, 3)))].join(" "));
let boom = false;
class I0 { constructor() { this.i0 = 1; } }
class I1 extends I0 { f = (() => { if (boom) throw new Error("f"); return 2; })(); constructor() { try { super(); } catch (e) { this.caught = e.message; } } }
const i1 = JSON.stringify(rep(() => new I1()));
boom = true;
print([i1, JSON.stringify(rep(() => new I1()))].join(" "));
class A { constructor(x) { this.a = x; } }
class D3 extends A { constructor() { super(1); try { super(2); } catch (e) { this.second = e.constructor.name; } } }
class TB extends A { constructor() { let r = "none"; try { const t = this; r = typeof t; } catch (e) { r = e.constructor.name; } super(r); } }
print([JSON.stringify(rep(() => new D3())), JSON.stringify(rep(() => new TB()))].join(" "));
"#;
    assert_eq!(
        printed(source, "derived-construct-lane-steps.js"),
        [
            r#"{"s":2000,"t":4000} true {"s":2001,"n":3}"#,
            r#"{"i0":1,"f":2} {"i0":1,"caught":"f"}"#,
            r#"{"a":1,"second":"ReferenceError"} {"a":"ReferenceError"}"#,
        ]
    );
}

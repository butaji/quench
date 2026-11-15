use super::*;

#[test]
fn lane_polymorphic_field_hints_keep_lookup_semantics() {
    // A polymorphic read keeps its last own entry as its record's hint: receivers of one
    // shape, then alternating ones, a site a prototype's entry answers, which drops the hint,
    // and receivers whose field changes value, shape or holder all read what the general
    // lookup reads, from a plain record and a returning one.
    let source = r#"
      const out = [];
      function get(o) { return o.x; }
      function read(o) { const v = o.x; return v + 1; }
      const os = [{ x: 1, a: 0 }, { x: 2, b: 0 }, { x: 3, c: 0 }];
      let s = 0;
      for (let r = 0; r < 200; r++) for (let k = 0; k < 3; k++) { const o = os[r < 100 ? 0 : k]; s += get(o) + read(o); }
      out.push(s);
      class B {} B.prototype.x = 40;
      const held = [new B(), { x: 5, d: 0 }, new B()];
      s = 0; for (let r = 0; r < 100; r++) for (const o of held) s += read(o) + get(o);
      out.push(s);
      os[0].x = "s"; delete os[1].b; os[1].x = 7; B.prototype.x = 1;
      out.push(os.map(get).join(","), os.map(read).join(","), held.map(read).join(","));
      print(out.join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "lane-polymorphic-hints.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["2400 17300 s,7,3 s1,8,4 2,6,2"]);
}

#[test]
fn lane_global_undefined_reads_keep_binding_semantics() {
    // A static global read of `undefined` answers its value in the lane, while a direct
    // eval's `var undefined` and a `with` object's property still shadow it, and neither an
    // eval's own `let` nor a write to the global changes it.
    let source = r#"
      const out = [];
      function f() { eval("var undefined = 1"); let s; for (let i = 0; i < 3; i++) s = undefined; return s; }
      function g(o) { with (o) { let s; for (let i = 0; i < 3; i++) s = undefined; return s; } }
      function h() { let s = 0; for (let i = 0; i < 1000; i++) s = typeof undefined + (undefined === void 0); return s; }
      out.push(f(), g({ undefined: "w" }), g({}), h());
      (0, eval)("let undefined = 3");
      globalThis.undefined = 7; out.push(h());
      print(out.join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "lane-global-undefined.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["1 w  undefinedtrue undefinedtrue"]);
}

#[test]
fn lane_field_reads_of_strings_and_hashed_sites_keep_lookup_semantics() {
    // A string's `length` read in the lane answers its own length, whatever its chain holds,
    // while other receivers at the same site read theirs; a site past its inline shapes
    // answers own and prototype fields from its hashed entries and sees a prototype change.
    let source = r#"
      const out = [];
      class T { constructor(s) { this.input = s; } len() { return this.input.length; } }
      let n = 0; const t = new T("abcdef");
      for (let i = 0; i < 1000; i++) n += t.len();
      out.push(n);
      t.input = [1, 2]; out.push(t.len());
      t.input = { length: "own" }; out.push(t.len());
      String.prototype.length = 99; out.push(new T("xy").len());
      const shapes = [];
      for (let k = 0; k < 12; k++) { const o = { ["f" + k]: k, type: "t" + k }; shapes.push(o); }
      function Proto() {} Proto.prototype.type = "proto";
      for (let k = 0; k < 3; k++) { const o = new Proto(); o["g" + k] = k; shapes.push(o); }
      function readType(o) { return o.type; }
      for (let r = 0; r < 50; r++) for (const o of shapes) readType(o);
      out.push(shapes.map(readType).join(","));
      Proto.prototype.type = "changed"; out.push(readType(shapes[13]));
      print(out.join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "lane-field-reads.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["6000 2 own 2 t0,t1,t2,t3,t4,t5,t6,t7,t8,t9,t10,t11,proto,proto,proto changed"]
    );
}

#[test]
fn lane_calls_passing_extra_values_keep_them_only_for_legacy_reflection() {
    // A lane call passing more values than its callee's parameters keeps them for a sloppy
    // callee, whose `f.arguments` and caller's `arguments` read them, and passes a strict
    // function, an arrow or a method its parameters alone (`drops_extra_arguments`).
    let source = r#"
      function sloppy(a) { return sloppy.arguments.length + ":" + Array.prototype.join.call(sloppy.arguments, ","); }
      function viaCaller(a) { return inner(); }
      function inner() { return Array.prototype.join.call(inner.caller.arguments, ","); }
      function strict(a, b) { "use strict"; return a + b; }
      const arrow = (a) => a * 2;
      const object = { method(a) { return a + 1; } };
      function run() {
        let out = "";
        let sum = 0;
        for (let i = 0; i < 2000; i++) {
          out = sloppy(i, 2, 3) + "|" + viaCaller(i, 5, 6);
          sum += strict(i, 1, 2, 3) + arrow(i, 9, 9) + object.method(i, 8);
        }
        return out + "|" + sum;
      }
      print(run());
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "lane-extra-values.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["3:1999,2,3|1999,5,6|8000000"]);
}

#[test]
fn hot_bodies_construct_forwarding_classes_from_nested_calls() {
    // A hot function builds vectors through a forwarding constructor while its own callers
    // are native: the construction resolves its target and keeps the forwarded fields.
    let source = r#"
      var Class = { create: function () { return function () { this.initialize.apply(this, arguments); }; } };
      var V = Class.create();
      V.prototype = {
        initialize: function (x, y, z) { this.x = x; this.y = y; this.z = z; },
        cross: function (w) { return new V(this.y * w.z - this.z * w.y, this.z * w.x - this.x * w.z, this.x * w.y - this.y * w.x); },
        dot: function (w) { return this.x * w.x + this.y * w.y + this.z * w.z; }
      };
      function shade(a, b) { var c = a.cross(b); var d = c.cross(a); return c.dot(b) + d.dot(a); }
      function trace(n) {
        var s = 0;
        for (var i = 0; i < n; i++) { s += shade(new V(i, 2, 3), new V(1, i, 5)); }
        return s;
      }
      var r = 0;
      for (var k = 0; k < 40; k++) { r += trace(200); }
      print(r);
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "hot-forwarding-construct.js").unwrap();
    vm.execute(&program).unwrap();
    let mut expected = 0i64;
    for i in 0..200i64 {
        let (a, b) = ((i, 2, 3), (1, i, 5));
        let cross = |p: (i64, i64, i64), q: (i64, i64, i64)| (p.1 * q.2 - p.2 * q.1, p.2 * q.0 - p.0 * q.2, p.0 * q.1 - p.1 * q.0);
        let dot = |p: (i64, i64, i64), q: (i64, i64, i64)| p.0 * q.0 + p.1 * q.1 + p.2 * q.2;
        let c = cross(a, b);
        let d = cross(c, a);
        expected += dot(c, b) + dot(d, a);
    }
    assert_eq!(output.borrow().as_slice(), [(expected * 40).to_string()]);
}

#[test]
fn forwarded_construct_plans_retire_when_their_prototype_or_method_changes() {
    // A plan made while hot holds under one dependency: a new method value, a new
    // prototype object and a replaced method each take the next construction off it.
    let source = r#"
      var Class = { create: function () { return function () { this.initialize.apply(this, arguments); }; } };
      var V = Class.create();
      V.prototype = { initialize: function (x, y) { this.x = x; this.y = y; } };
      function make(i) { return new V(i, 2); }
      function run(n) { var s = 0; for (var i = 0; i < n; i++) { var v = make(i); s += v.x + v.y; } return s; }
      function warm() { for (var k = 0; k < 40; k++) { run(300); } }
      var out = [];
      warm(); out.push(run(10));
      V.prototype.initialize = function (x, y) { this.x = x * 10; this.y = y; };
      out.push(run(10));
      warm();
      V.prototype = { initialize: function (x, y) { this.x = -x; this.y = y; } };
      out.push(run(10));
      warm();
      V.prototype.initialize = function (x, y) { this.x = x; this.y = 100; };
      out.push(run(10));
      print(out.join());
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "retiring-construct-plans.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["65,470,-25,1045"]);
}

#[test]
fn hot_bodies_construct_through_forwarded_initializers_that_do_more_than_store() {
    // A hot function builds objects through a forwarding constructor whose initializer calls
    // a function and returns an object: the construction yields the receiver whatever the
    // initializer returns, and a prototype change after the bodies are hot takes the next
    // construction off the native path.
    let source = r#"
      var Class = { create: function () { return function () { this.initialize.apply(this, arguments); }; } };
      var C = Class.create();
      function scale(v) { return v * 3; }
      C.prototype = { initialize: function (x, y) { this.x = scale(x); this.y = y + 1; return { x: -1 }; } };
      function make(i) { return new C(i, 2); }
      function run(n) { var s = 0; for (var i = 0; i < n; i++) { var c = make(i); s += c.x + c.y; } return s; }
      var out = [];
      for (var k = 0; k < 60; k++) { run(300); }
      out.push(run(10));
      C.prototype.initialize = function (x, y) { this.x = x; this.y = 0; };
      out.push(run(10));
      print(out.join());
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "hot-forwarded-initializer.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["165,45"]);
}

#[test]
fn lane_bodies_join_unboxed_parameters_that_a_field_read_proves_objects() {
    // `x` is an int32 or float parameter on some entries and an object on others: the unit
    // joins its unboxed view with the object proof of `x['a']` and must keep lowering
    // without a tagged view for it (it once reached an unreachable arm).
    let source = r#"
      function f1(x, y, i) { let s = 0; let k = i & 7; for (let j = 0; j < 5; j++) s = (s + ((((x|0) != (x*y)) + (x)['a']) || ((y?(x*y):k) != (3 ^ (x%5))))); return s; }
      function P(v) { this.v = v; }
      const A = [-3, new P(3), 1.5, NaN, [1, 2, 3]], B = [1.5, -2147483648, { a: 1 }, 7, null];
      let h = 0;
      for (let i = 0; i < 3500; i++) { try { h = (h * 31 + String(f1(A[i % 5], B[(i >> 1) % 5], i)).length) | 0; } catch (e) { h = (h * 31 + 1) | 0; } }
      print(h);
    "#;
    let run = |tiers: bool| {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        if tiers { vm.programs.enable_js_lane_methods(); }
        let program = Engine::specialize(source, "tagged-view-join.js").unwrap();
        vm.execute(&program).unwrap();
        output.borrow().clone()
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn lane_constructions_forward_plain_loaded_arguments() {
    // A loop the lane runs constructs through a forwarding constructor
    // (`this.initialize.apply(this, arguments)`): its frameless activation reads the
    // unmaterialized `arguments`, never a local of the frame the lane runs in.
    let source = r#"
      var Class = { create: function () { return function () { this.initialize.apply(this, arguments); }; } };
      var P = Class.create();
      P.prototype = { initialize: function (a, b) { this.a = a; this.b = b; } };
      var Q = Class.create();
      Q.prototype = { initialize: function () { this.n = arguments.length; this.first = arguments[0]; } };
      function run() {
        var s = 0;
        for (var i = 0; i < 2000; i++) { var p = new P(i, 2); s += p.a + p.b; }
        return s;
      }
      function run2() {
        var s = 0;
        for (var i = 0; i < 2000; i++) { var q = new Q(i, 1, 2); s += q.n + q.first; }
        return s;
      }
      print(run() + " " + run2());
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "lane-forwarding-construct.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["2003000 2005000"]);
}

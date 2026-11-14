//! Property attributes read through shapes: every attribute change, accessor and data
//! redefinition, deletion, freezing, prototype shadowing, dictionary mode and proxied `in` keeps
//! the answers Node gives, in each execution mode, and a shape that is read long earns its index.
use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// Each case prints one line; the expected lines are Node 24's.
const CASES: &str = r#"(function () {
  var out = typeof print === "function" ? print : console.log;
  function d(o, k) { return JSON.stringify(Object.getOwnPropertyDescriptor(o, k), function (_, v) { return typeof v === "function" ? "fn" : v; }); }
  // defineProperty attribute changes on a shape chain with many transitions
  var a = { p0: 0, p1: 1, p2: 2, p3: 3, p4: 4, p5: 5, p6: 6 };
  Object.defineProperty(a, "p2", { enumerable: false });
  Object.defineProperty(a, "p4", { writable: false });
  Object.defineProperty(a, "p5", { configurable: false });
  var sum = 0;
  for (var i = 0; i < 400; i++) { a.p4 = i; a.p3 = i; sum += a.p3 + a.p4 + a.p2; }
  out([sum, d(a, "p2"), d(a, "p4"), d(a, "p5"), Object.keys(a).join(",")].join("|"));
  out(delete a.p5 + "," + delete a.p2 + "," + Object.keys(a).join(","));
  // accessor to data and back, read hot
  var o = { x: 1, y: 2, z: 3 };
  function readX(obj) { return obj.x; }
  var t = 0;
  for (var i = 0; i < 300; i++) t += readX(o);
  Object.defineProperty(o, "x", { get: function () { return 10; }, configurable: true });
  for (var i = 0; i < 300; i++) t += readX(o);
  Object.defineProperty(o, "x", { value: 7, writable: true, configurable: true, enumerable: true });
  for (var i = 0; i < 300; i++) t += readX(o);
  Object.defineProperty(o, "x", { get: function () { return 100; }, set: function (v) { t += v; }, configurable: true });
  o.x = 5;
  for (var i = 0; i < 300; i++) t += readX(o);
  out([t, d(o, "x"), Object.keys(o).join(",")].join("|"));
  // delete and re-add
  var r = { a: 1, b: 2, c: 3 };
  function readB(obj) { return obj.b; }
  var u = 0;
  for (var i = 0; i < 300; i++) u += readB(r);
  delete r.b;
  var gone = String(readB(r));
  r.b = 9;
  for (var i = 0; i < 300; i++) u += readB(r);
  r.d = 4; delete r.a; r.a = 5;
  out([u, gone, Object.keys(r).join(","), JSON.stringify(r)].join("|"));
  // frozen and sealed
  var f = Object.freeze({ k: 1, m: 2 }), s = Object.seal({ k: 1, m: 2 });
  f.k = 5; s.k = 5; s.n = 1; f.n = 1;
  var fs = 0;
  for (var i = 0; i < 300; i++) { fs += f.k + s.k; }
  out([fs, d(f, "k"), d(s, "k"), Object.isFrozen(f), Object.isSealed(s), delete s.k, delete f.k].join("|"));
  // prototype chains with shadowing
  var P = { get v() { return "pg"; }, w: "pw" };
  var Q = Object.create(P); Q.extra = 1;
  var R = Object.create(Q);
  function readV(obj) { return obj.v + obj.w; }
  var line = [];
  for (var i = 0; i < 300; i++) readV(R);
  line.push(readV(R));
  Object.defineProperty(Q, "v", { value: "qv", configurable: true });
  line.push(readV(R));
  R.v = "own-ignored-if-readonly";
  line.push(readV(R));
  Object.defineProperty(R, "w", { get: function () { return "rg"; } });
  line.push(readV(R));
  delete Q.v;
  line.push(readV(R));
  out(line.join("|"));
  // dictionary mode
  var big = {};
  for (var i = 0; i < 300; i++) big["k" + i] = i;
  for (var i = 0; i < 300; i += 3) delete big["k" + i];
  Object.defineProperty(big, "k1", { enumerable: false, value: 11 });
  Object.defineProperty(big, "k2", { get: function () { return "acc"; }, configurable: true });
  var bs = 0;
  function readBig(o) { return o.k1 + o.k2 + o.k4; }
  for (var i = 0; i < 300; i++) bs += readBig(big).length;
  out([bs, Object.keys(big).length, d(big, "k1"), d(big, "k2"), "k3" in big, "k0" in big].join("|"));
  // `in` with proxies
  var log = [];
  var px = new Proxy({ a: 1 }, { has: function (t, k) { log.push("has:" + String(k)); return k === "z" || k in t; } });
  var child = Object.create(px);
  out(["a" in px, "z" in px, "q" in px, "a" in child, "z" in child, "own" in Object.assign(Object.create(px), { own: 1 }), log.join(",")].join("|"));
  // getter-returning methods and local-base reads
  function Box(v) { this.v = v; }
  Box.prototype.get = function () { return this.v; };
  Object.defineProperty(Box.prototype, "twice", { get: function () { return this.v * 2; }, configurable: true });
  Object.defineProperty(Box.prototype, "same", { get: function () { return this.v; }, configurable: true });
  function usesLocal(n) {
    var b = new Box(n), acc = 0;
    for (var i = 0; i < 400; i++) { acc += b.v + b.twice + b.same + b.get(); }
    return acc;
  }
  var lr = [usesLocal(1), usesLocal(2)];
  Object.defineProperty(Box.prototype, "twice", { value: 1000, configurable: true });
  lr.push(usesLocal(3));
  Object.defineProperty(Box.prototype, "same", { get: function () { return 7; }, configurable: true });
  lr.push(usesLocal(4));
  Box.prototype.v = 50;
  function noOwn() { var b = Object.create(Box.prototype), acc = 0; for (var i = 0; i < 300; i++) acc += b.v; return acc; }
  lr.push(noOwn());
  out(lr.join("|"));
  // polymorphic sites: nested reads, prototype-held fields of many receiver shapes, functions
  var kinds = [];
  for (var k = 0; k < 40; k++) {
    var C = new Function("this.id = " + k + ";");
    if (k % 3 === 0) C.prototype.extra = k;
    if (k % 5 === 0) C.prototype.constructor = Object;
    kinds.push(new C());
  }
  function ctorName(o) { return o.constructor.name; }
  var names = [];
  for (var rep = 0; rep < 60; rep++) for (var k = 0; k < kinds.length; k++) { var n = ctorName(kinds[k]); if (rep === 59) names.push(n); }
  out(names.join(","));
  Object.setPrototypeOf(kinds[1], kinds[0]);
  Object.setPrototypeOf(kinds[2], Array.prototype);
  kinds[3].constructor = Map;
  out([ctorName(kinds[1]), ctorName(kinds[2]), ctorName(kinds[3]), ctorName(kinds[4])].join(","));
  var fns = [];
  for (var k = 0; k < 40; k++) fns.push(new Function("a", "return a + " + k + ";"));
  function callIt(f) { return f.call(null, 1); }
  var cs = 0;
  for (var rep = 0; rep < 60; rep++) for (var k = 0; k < fns.length; k++) cs += callIt(fns[k]);
  fns[7].call = function () { return "own"; };
  Object.defineProperty(fns[8], "call", { get: function () { return function () { return "acc"; }; } });
  var saved = Function.prototype.call;
  out([cs, callIt(fns[7]), callIt(fns[8]), callIt(fns[9])].join(","));
  Function.prototype.call = function () { return "patched"; };
  out(callIt(fns[10]) + "," + callIt(fns[7]));
  Function.prototype.call = saved;
  out(callIt(fns[10]));
  var arrs = [[1, 2, 3], [], new Array(5)];
  function len(a) { return a.length; }
  var ls = 0;
  for (var rep = 0; rep < 300; rep++) for (var k = 0; k < arrs.length; k++) ls += len(arrs[k]);
  Object.defineProperty(arrs[0], "length", { writable: false });
  arrs[1].push(4, 5);
  out([ls, len(arrs[0]), len(arrs[1]), len({ length: 9 }), len("abcd"), len(function (a, b) {})].join(","));
  // returning getter
  var G = { get g() { return this.base + 1; }, base: 1 };
  function retG(o) { return o.g; }
  var rg = 0;
  for (var i = 0; i < 300; i++) rg += retG(G);
  G.base = 5;
  rg += retG(G);
  Object.defineProperty(G, "g", { value: 3 });
  rg += retG(G);
  out(rg);
})();
"#;

const EXPECTED: &[&str] = &[
    r#"82200|{"value":2,"writable":true,"enumerable":false,"configurable":true}|{"value":4,"writable":false,"enumerable":true,"configurable":true}|{"value":5,"writable":true,"enumerable":true,"configurable":false}|p0,p1,p3,p4,p5,p6"#,
    r#"false,true,p0,p1,p3,p4,p5,p6"#,
    r#"35405|{"get":"fn","set":"fn","enumerable":true,"configurable":true}|x,y,z"#,
    r#"3300|undefined|c,b,d,a|{"c":3,"b":9,"d":4,"a":5}"#,
    r#"1800|{"value":1,"writable":false,"enumerable":true,"configurable":false}|{"value":5,"writable":true,"enumerable":true,"configurable":false}|true|true|false|false"#,
    r#"pgpw|qvpw|qvpw|qvrg|pgrg"#,
    r#"1800|199|{"value":11,"writable":true,"enumerable":false,"configurable":true}|{"get":"fn","enumerable":true,"configurable":true}|false|false"#,
    r#"true|true|false|true|true|true|has:a,has:z,has:q,has:a,has:z"#,
    r#"2000|4000|403600|406000|15000"#,
    r#"Object,anonymous,anonymous,anonymous,anonymous,Object,anonymous,anonymous,anonymous,anonymous,Object,anonymous,anonymous,anonymous,anonymous,Object,anonymous,anonymous,anonymous,anonymous,Object,anonymous,anonymous,anonymous,anonymous,Object,anonymous,anonymous,anonymous,anonymous,Object,anonymous,anonymous,anonymous,anonymous,Object,anonymous,anonymous,anonymous,anonymous"#,
    r#"Object,Array,Map,anonymous"#,
    r#"49200,own,acc,10"#,
    r#"patched,own"#,
    r#"11"#,
    r#"2400,3,2,9,4,2"#,
    r#"609"#,
];

#[test]
fn attribute_cases_match_node_in_every_execution_mode() {
    let program = Engine::specialize(CASES, "shape-attribute-cases.js").unwrap();
    for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.set_execution_mode(mode);
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), EXPECTED, "{mode:?}");
    }
}

#[test]
fn long_attribute_walks_earn_the_shape_a_lookup_index_with_the_same_answers() {
    let source = "var o = { p0: 0, p1: 1, p2: 2, p3: 3, p4: 4, p5: 5, p6: 6, p7: 7, p8: 8 };\n\
         Object.defineProperty(o, 'p0', { enumerable: false });\n\
         Object.defineProperty(o, 'p2', { writable: false });";
    let program = Engine::specialize(source, "shape-attribute-index.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("o");
    let object = vm.own_property(vm.realm.globals, name).unwrap();
    let shape = vm.object_data(object).unwrap().shape();
    let attributes = |vm: &mut Vm<TestHost>, name: &str| {
        let atom = vm.intern_atom(name);
        let (_, slot) = vm.object_property_slot(object, PropertyKey::string(atom)).unwrap();
        let attributes = vm.shape_slot_attributes(shape, slot).unwrap();
        (attributes.writable, attributes.enumerable, attributes.configurable)
    };
    assert!(vm.shapes[shape as usize].lookup_index.get().is_none());
    let walked: Vec<_> = ["p0", "p1", "p2", "p8"].iter().map(|name| attributes(&mut vm, name)).collect();
    assert_eq!(walked, [(true, false, true), (true, true, true), (false, true, true), (true, true, true)]);
    for _ in 0..u8::MAX {
        attributes(&mut vm, "p1");
    }
    assert!(vm.shapes[shape as usize].lookup_index.get().is_some());
    let indexed: Vec<_> = ["p0", "p1", "p2", "p8"].iter().map(|name| attributes(&mut vm, name)).collect();
    assert_eq!(indexed, walked);
}

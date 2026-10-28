use super::wtf16::JsString;
use super::{
    CallContext, CallTarget, IteratorRealmPrototypes, JsError, MethodCache, NO_FIELD_HOLDER,
    Native, TypedArrayKind, Vm, activation::Completion, activation::Continuation,
    regexp::RegExpIntrinsics,
};
use crate::{Engine, Value};
use crate::test_host::TestHost;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn collection_construction_survives_gc_in_iterator_and_entry_callbacks() {
    let source = r#"
      const out=[];
      for (const C of [Map, Set, WeakMap, WeakSet]) {
        const pair=C===Map || C===WeakMap;
        const key={rank:1}, value={answer:42}; let once=false, observed;
        const method=pair?'set':'add';
        const descriptor=Object.getOwnPropertyDescriptor(C.prototype,method);
        Object.defineProperty(C.prototype,method,{configurable:true,get(){
          $262.gc();
          return function(k,v){observed=k;$262.gc();return Reflect.apply(descriptor.value,this,pair?[k,v]:[k])};
        }});
        const iterable={ [Symbol.iterator]() {
          $262.gc();
          return {next() {
            $262.gc();
            if (once) return {done:true}; once=true;
            return {done:false,value:pair?{
              get 0() {$262.gc();return {rank:1}},
              get 1() {$262.gc();return value}
            }:key};
          }};
        }};
        const result=new C(iterable);
        Object.defineProperty(C.prototype,method,descriptor);
        out.push(pair?result.get(observed)===value && observed.rank===1:result.has(key) && observed===key);
      }
      let closed=0;
      try { new Map({[Symbol.iterator](){return {next(){return {value:17,done:false}},return(){$262.gc();closed++;return {done:true}}}}}); }
      catch(error) {out.push(error.name,closed);}
      if (JSON.stringify(out)!=='[true,true,true,true,"TypeError",1]') throw new Error(JSON.stringify(out));
    "#;
    for specialized in [false, true] {
        let mut vm = Vm::new(TestHost::default().with_test262());
        let program = if specialized {
            Engine::specialize(source, "collection-constructor-gc.js")
        } else {
            Engine::compile(crate::ExecutionRequest {
                source,
                name: "collection-constructor-gc.js",
                kind: crate::SourceKind::Script,
            })
        }
        .unwrap();
        vm.execute(&program).unwrap();
    }
}

#[test]
fn whole_read_per_iteration_copies_keep_their_owners_slots() {
    // Bindings enough that each iteration's copy of the script's root environment is sparse
    // (`SPARSE_CLONE_MIN_SLOTS`); the root holds projections, so its copies are traced whole,
    // and `eval` makes a closure read its chain whole.
    const BINDINGS: usize = crate::heap::SPARSE_CLONE_MIN_SLOTS + 8;
    let bindings = (0..BINDINGS).map(|index| format!("let a{index}={{v:{index}}};")).collect::<String>();
    let source = format!(
        r#"
      {bindings}
      const fs=[];
      for (let i=0;i<4;i++) {{ const own={{i}}; fs.push(() => a5.v + a60.v + own.i + i, () => eval('a5.v + a60.v + own.i + i')); }}
      $262.gc(); $262.gc();
      const got=JSON.stringify(fs.map(f=>f()));
      if (got!=='[65,65,67,67,69,69,71,71]') throw new Error(got);
    "#
    );
    for specialized in [false, true] {
        let mut vm = Vm::new(TestHost::default().with_test262());
        let program = if specialized {
            Engine::specialize(&source, "sparse-whole-read.js")
        } else {
            Engine::compile(crate::ExecutionRequest {
                source: &source,
                name: "sparse-whole-read.js",
                kind: crate::SourceKind::Script,
            })
        }
        .unwrap();
        vm.execute(&program).unwrap();
    }
}

#[test]
fn array_definitions_reuse_accessor_slots_and_allow_frozen_same_values() {
    let source = r#"
      for(const index of [0,100000]) {
        const array=[];let reads=0;
        Object.defineProperty(array,index,{get(){reads++;return 7},configurable:true,enumerable:true});
        Object.preventExtensions(array);
        Object.defineProperty(array,index,{value:42,writable:true});
        if(array[index]!==42||reads!==0)throw Error('accessor conversion');
        Object.freeze(array);
        if(!Reflect.defineProperty(array,index,{value:42}))throw Error('compatible frozen value');
        if(Reflect.defineProperty(array,index,{value:43}))throw Error('incompatible frozen value');
        if(Reflect.set(array,index,42)||Reflect.set(array,index,43))throw Error('frozen assignment');
        const descriptor=Object.getOwnPropertyDescriptor(array,index);
        if(descriptor.value!==42||descriptor.writable||descriptor.configurable||!descriptor.enumerable)throw Error('frozen descriptor');
      }
    "#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "array-definition-integrity.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source,"array-definition-integrity.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn strict_terminal_calls_preserve_catch_and_finally_handlers() {
    let source = r#"
      'use strict';
      function bad() { throw 17; }
      function caught() { try { return bad(); } catch (error) { return error + 1; } }
      function nested() {
        try { return bad(); }
        catch (error) { try { return bad(); } catch (inner) { return inner + error; } }
      }
      const events = [];
      function finalized() {
        try { return bad(); }
        catch (error) { return error + 2; }
        finally { events.push('finally'); }
      }
      function nativeError() { try { return JSON.parse(']'); } catch (error) { return error.name; } }
      function protectedCall(fn) { try { return fn(); } catch (error) { return error + 3; } }
      print(JSON.stringify([caught(), nested(), finalized(), nativeError(), protectedCall(bad), events]));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "strict-terminal-handlers.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        [r#"[18,34,19,"SyntaxError",20,["finally"]]"#]
    );
}

#[test]
fn lane_generator_iteration_steps_keep_generator_semantics() {
    // A for-of step of an intrinsic generator `next` resumes the generator in place with a
    // reused result (`lane_generator_step`): delegation passes its inner result on, a throw
    // and a break close as before, a patched `next`, a running generator and a done one take
    // the general step, and nested and repeated loops see their own values.
    let source = r#"
      const out = [];
      function* range(n) { for (let i = 0; i < n; i++) yield i; }
      function* inner() { yield "a"; yield "b"; return "r"; }
      function* deleg() { const r = yield* inner(); yield r; }
      function* map(it, f) { for (const x of it) yield f(x); }
      function* thrower() { yield 1; throw new Error("boom"); }
      function* fin() { try { yield 1; yield 2; } finally { out.push("fin"); } }
      let s = 0; for (const x of range(1000)) s += x; out.push(s);
      const d = []; for (const x of deleg()) d.push(x); out.push(d.join(""));
      s = 0; for (const x of map(map(range(100), (x) => x * 2), (x) => x + 1)) s += x; out.push(s);
      try { for (const x of thrower()) out.push("t" + x); } catch (e) { out.push(e.message); }
      for (const x of fin()) { out.push("f" + x); break; }
      const g = range(3); g.next(); for (const x of g) out.push("rest" + x);
      function* self() { const me = yield 1; try { for (const x of me) {} } catch (e) { out.push(e.constructor.name); } }
      const it = self(); it.next(); it.next(it);
      let patchedSteps = 0;
      const gen = range(2); gen.next = function () { return { value: "patched", done: ++patchedSteps > 2 }; };
      for (const x of gen) out.push(x);
      const done = range(2); for (const x of done) {} out.push(JSON.stringify(done.next()));
      print(out.join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "lane-generator-steps.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["499500 abr 10000 t1 boom f1 fin rest1 rest2 TypeError patched patched {\"done\":true}"]
    );
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

// Profiled builds count interpreted work exactly and hold no hints to clear.
#[cfg(not(quench_profiled))]
#[test]
fn shape_compaction_clears_own_field_hints() {
    // Read sites warmed on one layout hold its shape id as their own-field hint. A full
    // collection that compacts the shape table renumbers ids, after which an old id can name
    // another layout, so the compaction clears every hint (`LaneView::clear_field_hints`).
    const SITES: usize = 64;
    let reads: String = (0..SITES).map(|k| format!("function read{k}(o) {{ return o.a + 0; }}\n")).collect();
    let calls: String = (0..SITES).map(|k| format!("read{k},")).collect();
    let source = format!(
        r#"
      {reads}
      var sites = [{calls}], sum = 0;
      function T(a, b) {{ this.a = a; this.b = b; }}
      function drive(read, o, n) {{ var s = 0; for (var i = 0; i < n; i++) s += read(o); return s; }}
      for (var k = 0; k < sites.length; k++) sum += drive(sites[k], new T(k, -1), 50);
      // Layouts no object keeps, so the next full collection compacts the table.
      for (var k = 0; k < 20000; k++) {{ var o = {{}}; o['junk' + k] = k; }}
      print(String(sum));
    "#
    );
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(&source, "field-hints-compaction.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), [(50 * (0..SITES).sum::<usize>()).to_string()]);
    let held = |vm: &Vm<TestHost>| {
        vm.programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .into_iter()
            .flatten()
            .filter_map(super::program_store::derived_lane_view)
            .map(|view| view.held_field_hints())
            .sum::<usize>()
    };
    assert!(held(&vm) >= SITES);
    let shapes = vm.shapes.len();
    vm.collect_now(&program);
    assert!(vm.shapes.len() < shapes, "the collection compacted the shape table");
    assert_eq!(held(&vm), 0);
}

#[test]
fn call_sites_test_each_target_their_link_names() {
    // One call site links the target cache entry of a plain callee, which dies; after the
    // collection, functions that need another entry (a kept `arguments`, values past their
    // parameters) can take its handle, and so its entry. Each call still makes the tests its
    // own target needs. Expected values are Node's (v26). It guards the plain call's tests;
    // the trunk crash it follows (JetStream typescript-lib) it does not reproduce itself.
    let source = r#"
      function site(f, v) { return f(v, 2); }
      function plain(i) { return function (x, y) { return x + y + i; }; }
      var out = 0;
      for (var round = 0; round < 40; round++) {
        var f = plain(round);
        for (var k = 0; k < 60; k++) out += site(f, k) - k - 2 - round;
        f = null;
        $262.gc();
        for (var j = 0; j < 30; j++) {
          var keeps = function () { return arguments.length * 100 + arguments[1]; };
          var fewer = function (x) { return x; };
          out += site(keeps, j) - 202;
          out += site(fewer, j) - j;
        }
      }
      print(String(out));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()).with_test262());
    let program = Engine::specialize(source, "call-link-reuse.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["0"]);
}

#[test]
fn returned_forwarded_constructions_finish_in_the_lane() {
    // `return new V(…)` of forwarding constructors whose initializers are stores, from
    // activations and from the frame's own return, before and after the initializer is
    // replaced. Expected values are Node's (v26).
    let source = r#"
      "use strict";
      var Class = { create: function() { return function() { this.initialize.apply(this, arguments); }; } };
      var V = Class.create();
      V.prototype = { initialize: function(x, y, z) { this.x = (x ? x : 0); this.y = (y ? y : 0); this.z = (z ? z : 0); },
        add: function(o) { return new V(this.x + o.x, this.y + o.y, this.z + o.z); } };
      var C = Class.create();
      C.prototype = { initialize: function(r) { if (!r) r = 0.5; this.r = r; } };
      function mk(k) { return new C(k % 3); }
      function deep(k) { return k ? deep(k - 1) : new V(k, 1); }
      var a = new V(1, 2, 3), s = 0, out = [];
      for (var i = 0; i < 5000; i++) { a = a.add(new V(i % 2, 0, -1)); s += mk(i).r; }
      out.push(JSON.stringify(a), s, JSON.stringify(deep(20)), JSON.stringify(mk(0)), Object.keys(a.add(a)).join());
      V.prototype.initialize = function(x) { this.q = x; };
      out.push(JSON.stringify(a.add(a)));
      print(out.join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "returned-forwarded-construct.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        [r#"{"x":2501,"y":2,"z":-4997} 5832.5 {"x":0,"y":1,"z":0} {"r":0.5} x,y,z {"q":5002}"#]
    );
}

#[test]
fn planned_constructions_run_forwarded_initializers_and_their_defaults() {
    // One `new` site constructs an initializer constructor, whose plan it allocates from, and
    // forwarding constructors, whose plans hold only with their forwarding, which a getter
    // keeps from resolving: each gets the fields its initializer stores, falsy arguments
    // taking the Number defaults, and a replaced initializer runs in place of the planned
    // one. Expected values are Node's (v26).
    let source = r#"
      var Class = { create: function () { return function () { this.initialize.apply(this, arguments); }; } };
      function A(x) { this.x = x; }
      var B = Class.create();
      B.prototype = { initialize: function (x) { this.y = x + 1; } };
      var C = Class.create();
      C.prototype = { initialize: function (x, y) { this.z = (x ? x : 4); if (!y) y = 0.5; this.w = y; } };
      var D = Class.create();
      D.prototype = Object.defineProperty({}, "initialize", { get: function () { return function (x) { this.d = x; }; } });
      var out = [];
      for (var i = 0; i < 3000; i++) { var o = new [A, B, C, D][i % 4](i % 5, i % 2 ? "s" : ""); if (i > 2995) out.push(JSON.stringify(o)); }
      var falsy = [0, -0, "", NaN, null, undefined, false, 0n];
      for (var i = 0; i < falsy.length; i++) { var c = new C(falsy[i], falsy[i]); out.push(c.z + "/" + c.w); }
      C.prototype.initialize = function (x) { this.v = x; };
      out.push(JSON.stringify(new C(7)));
      print(out.join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "planned-forwarded-construct.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        [r#"{"x":1} {"y":3} {"z":3,"w":0.5} {"d":4} 4/0.5 4/0.5 4/0.5 4/0.5 4/0.5 4/0.5 4/0.5 4/0.5 {"v":7}"#]
    );
}



#[test]
fn promoted_locals_preserve_order_and_survive_collection() {
    let source = r#"
      function read(value) {
        var index = 0;
        while (index < 5000) {
          var discarded = {};
          index++;
        }
        return value.answer;
      }
      function ordered(value) { return value - (value = 2); }
      function destructured(value) {
        ({ field: value } = { field: 3 });
        return value;
      }
      function held(x, i) { var r = x[i] + x[++i] + x[++i]; return r * 10 + i; }
      function keyed(x, i) { x[i] = x[++i] * 10; return x.join() + ":" + i; }
      function window(i) { var a = [i, ++i, i++, i]; return a.join() + ":" + i; }
      function compound(i) { i += i++ + ++i; return i; }
      // A `var` read before its first write is undefined, whatever an earlier frame left
      // in the register it moved to.
      function dirty() { var a = "stale", b = "stale"; return a + b; }
      function unset(o) { var n; (function (e) { e.k = 1; })(n || (o.x = n = {})); return typeof o.x + ":" + n.k; }
      print(read({ answer: 42 }));
      print(ordered(5));
      print(destructured(9));
      print(held([10, 20, 30, 40], 0));
      print(keyed([1, 2, 3, 4], 0));
      print(window(0));
      print(compound(2));
      dirty();
      print(unset({}));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "promoted-parameter.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["42", "3", "3", "602", "20,2,3,4:1", "0,1,1,2:2", "8", "object:1"]
    );

    // Every binding here stays in a register, including those written inside expressions.
    assert_eq!(program.function(1).local_registers.len(), 3);
    assert_eq!(program.function(2).local_registers.len(), 1);
    assert_eq!(program.function(3).local_registers.len(), 1);
}

#[test]
fn fallback_descriptor_edges_follow_their_owner_lifetime() {
    use super::{DEFAULT_PROPERTY_ATTRIBUTES, property_key::PropertyKey};
    use crate::heap::Cell;
    let program = Engine::specialize("", "descriptor-owner.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    let owner = vm.heap.alloc(Cell::Array {
        object: crate::heap::ArrayObject::new(
            Vm::<TestHost>::empty_object(Value::NULL),
            Vec::new(),
        ),
    });
    let setter = vm.native_with_env(Native::Object, owner);
    let owner_weak = vm.heap.weak_handle(owner).unwrap();
    let setter_weak = vm.heap.weak_handle(setter).unwrap();
    let key = PropertyKey::string(vm.intern_atom("0"));
    vm.set_property_attributes(
        owner,
        key,
        super::PropertyAttributes {
            accessor: true,
            setter: Some(setter),
            ..DEFAULT_PROPERTY_ATTRIBUTES
        },
    );
    let root = vm.heap.root(owner);
    vm.collect_now(&program);
    assert_eq!(vm.heap.weak_value(owner_weak), Some(owner));
    assert_eq!(vm.heap.weak_value(setter_weak), Some(setter));
    assert!(vm.heap.release_root(root));
    let map = vm.heap.alloc(Cell::WeakMap {
        object: Box::new(Vm::<TestHost>::empty_object(Value::NULL)),
        entries: Box::default(),
    });
    let map_key = vm.object();
    if let Some(Cell::WeakMap { entries, .. }) = vm.heap.get_mut(map) {
        entries.insert(map_key, owner);
    }
    let map_root = vm.heap.root(map);
    let key_root = vm.heap.root(map_key);
    vm.collect_now(&program);
    assert_eq!(vm.heap.weak_value(owner_weak), Some(owner));
    assert_eq!(vm.heap.weak_value(setter_weak), Some(setter));
    assert!(vm.heap.release_root(key_root));
    vm.collect_now(&program);
    assert!(vm.heap.weak_value(owner_weak).is_none());
    assert!(vm.heap.weak_value(setter_weak).is_none());
    assert!(!vm.descriptors.contains_key(&(owner, key)));
    assert!(vm.heap.release_root(map_root));
}

#[test]
fn minor_collection_roots_external_edges_of_old_owners() {
    use super::{DEFAULT_PROPERTY_ATTRIBUTES, PropertyAttributes, property_key::PropertyKey};

    let program = Engine::specialize("", "minor-external-roots.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    let owner = vm.object();
    let owner_root = vm.heap.root(owner);
    vm.collect_slow(&program, true);

    let getter = vm.heap.alloc_string("young getter".into());
    let getter_weak = vm.heap.weak_handle(getter).unwrap();
    let key = PropertyKey::string(vm.intern_atom("field"));
    vm.set_property_attributes(
        owner,
        key,
        PropertyAttributes {
            getter: Some(getter),
            ..DEFAULT_PROPERTY_ATTRIBUTES
        },
    );

    vm.collect_slow(&program, false);
    assert_eq!(vm.heap.weak_value(getter_weak), Some(getter));

    vm.set_property_attributes(owner, key, DEFAULT_PROPERTY_ATTRIBUTES);
    vm.collect_slow(&program, true);
    assert!(vm.heap.weak_value(getter_weak).is_none());
    assert!(vm.heap.release_root(owner_root));
}

#[test]
fn minor_collection_keeps_accessor_pairs_added_to_old_owners() {
    use super::{PropertyAttributes, property_key::PropertyKey};

    let program = Engine::specialize("", "minor-accessor-add.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    let owner = vm.object();
    let owner_root = vm.heap.root(owner);
    vm.collect_slow(&program, true);

    let getter = vm.heap.alloc_string("young getter".into());
    let setter = vm.heap.alloc_string("young setter".into());
    let weak = [vm.heap.weak_handle(getter).unwrap(), vm.heap.weak_handle(setter).unwrap()];
    let key = PropertyKey::string(vm.intern_atom("binding"));
    vm.create_shape_accessor_property(
        owner,
        key,
        PropertyAttributes {
            writable: false,
            enumerable: true,
            configurable: false,
            accessor: true,
            getter: Some(getter),
            setter: Some(setter),
        },
    )
    .unwrap();

    vm.collect_slow(&program, false);
    assert_eq!(weak.map(|handle| vm.heap.weak_value(handle)), [Some(getter), Some(setter)]);
    assert!(vm.heap.release_root(owner_root));
}

#[test]
fn rootless_shapes_survive_collection_and_shape_compaction() {
    let program = Engine::specialize("", "shape-compaction.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    let atom = vm.intern_atom("survivor");
    let live_shape = vm.transition_shape(0, atom);
    assert!(!vm.shapes[live_shape as usize].facts.may_have_gc_roots());
    let owner =
        vm.heap
            .alloc_object_with_properties(Value::NULL, live_shape, &[Value::integer(42)]);
    let root = vm.heap.root(owner);
    // Dead transition history must be reclaimed even when the surviving
    // object's shape has no symbol keys or accessor functions to trace.
    let dead_shape_count = vm.shapes.len() * 2;
    for index in 0..dead_shape_count {
        let dead_atom = vm.intern_atom(&format!("dead-{index}"));
        vm.transition_shape(0, dead_atom);
    }
    let before = vm.shapes.len();
    vm.collect_now(&program);
    assert!(vm.shapes.len() < before);
    let object = vm.object_data(owner).unwrap();
    assert_eq!(vm.shapes[object.shape() as usize].storage_len(), 1);
    assert_eq!(
        vm.heap.property_get(vm.object_data(owner).unwrap(), 0),
        Some(Value::integer(42))
    );
    vm.collect_now(&program);
    assert_eq!(
        vm.heap.property_get(vm.object_data(owner).unwrap(), 0),
        Some(Value::integer(42))
    );
    assert!(vm.heap.release_root(root));
}

#[test]
fn shape_roots_keep_symbols_and_active_accessors_alive() {
    use super::{DEFAULT_PROPERTY_ATTRIBUTES, property_key::PropertyKey};
    use crate::heap::Cell;

    let program = Engine::specialize("", "shape-roots.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    let symbol = vm.heap.alloc(Cell::Symbol(crate::heap::SymbolData::public(None)));
    let symbol_weak = vm.heap.weak_handle(symbol).unwrap();
    let symbol_shape = vm.transition_property_shape(0, PropertyKey::symbol(symbol));
    let first =
        vm.heap
            .alloc_object_with_properties(Value::NULL, symbol_shape, &[Value::UNDEFINED]);
    let second =
        vm.heap
            .alloc_object_with_properties(Value::NULL, symbol_shape, &[Value::UNDEFINED]);
    let first_root = vm.heap.root(first);
    let second_root = vm.heap.root(second);

    vm.collect_now(&program);
    assert_eq!(vm.heap.weak_value(symbol_weak), Some(symbol));
    assert!(vm.heap.release_root(first_root));
    assert!(vm.heap.release_root(second_root));
    vm.collect_now(&program);
    assert!(vm.heap.weak_value(symbol_weak).is_none());

    let atom = vm.intern_atom("field");
    let data_shape = vm.transition_shape(0, atom);
    let owner = vm
        .heap
        .alloc_object_with_properties(Value::NULL, data_shape, &[Value::UNDEFINED]);
    let getter = vm.heap.alloc_string("getter".into());
    let getter_weak = vm.heap.weak_handle(getter).unwrap();
    vm.set_property_attributes(
        owner,
        PropertyKey::string(atom),
        super::PropertyAttributes {
            accessor: true,
            getter: Some(getter),
            ..DEFAULT_PROPERTY_ATTRIBUTES
        },
    );
    let owner_root = vm.heap.root(owner);
    vm.collect_now(&program);
    assert_eq!(vm.heap.weak_value(getter_weak), Some(getter));

    vm.set_property_attributes(
        owner,
        PropertyKey::string(atom),
        DEFAULT_PROPERTY_ATTRIBUTES,
    );
    vm.collect_now(&program);
    assert!(vm.heap.weak_value(getter_weak).is_none());
    assert!(vm.heap.release_root(owner_root));
}

#[test]
fn equivalent_data_descriptor_transitions_share_one_shape() {
    use super::property_key::PropertyKey;

    let program = Engine::specialize("", "descriptor-transition.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.initialize(&program).unwrap();
    vm.heap.retain_allocations_for_test();

    let atom = vm.intern_atom("field");
    let attributes = super::PropertyAttributes {
        writable: false,
        enumerable: false,
        configurable: true,
        accessor: false,
        getter: None,
        setter: None,
    };
    let first = vm.object();
    vm.set_property(first, atom, Value::integer(1)).unwrap();
    vm.set_property_attributes(first, PropertyKey::string(atom), attributes);
    let first_shape = vm.object_data(first).unwrap().shape();
    let shape_count = vm.shapes.len();

    let second = vm.object();
    vm.set_property(second, atom, Value::integer(2)).unwrap();
    vm.set_property_attributes(second, PropertyKey::string(atom), attributes);

    assert_eq!(vm.object_data(second).unwrap().shape(), first_shape);
    assert_eq!(vm.shapes.len(), shape_count);
}

#[test]
fn new_accessor_properties_reach_the_two_step_shape_in_one_add() {
    let program = Engine::specialize("", "accessor-add.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.initialize(&program).unwrap();
    vm.heap.retain_allocations_for_test();

    let key = super::property_key::PropertyKey::string(vm.intern_atom("binding"));
    let accessor = |getter, setter| super::PropertyAttributes {
        writable: false,
        enumerable: true,
        configurable: false,
        accessor: true,
        getter: Some(getter),
        setter,
    };
    let (first_getter, second_getter, setter) = (vm.object(), vm.object(), vm.object());
    let two_step = vm.object();
    vm.set_shape_property(two_step, key, Value::UNDEFINED).unwrap();
    vm.set_property_attributes(two_step, key, accessor(first_getter, None));
    let fused = vm.object();
    vm.create_shape_accessor_property(fused, key, accessor(second_getter, None)).unwrap();

    assert_eq!(vm.object_data(fused).unwrap().shape(), vm.object_data(two_step).unwrap().shape());
    assert!(vm.property_attributes(fused, key) == Some(accessor(second_getter, None)));
    assert!(vm.property_attributes(two_step, key) == Some(accessor(first_getter, None)));

    let both = vm.object();
    vm.create_shape_accessor_property(both, key, accessor(first_getter, Some(setter))).unwrap();
    assert!(vm.property_attributes(both, key) == Some(accessor(first_getter, Some(setter))));
}

#[test]
fn repeated_normal_closures_reuse_function_descriptor_shapes() {
    const CLOSURES: usize = 32;

    let program = Engine::specialize(
        "function outer() { return function plainName(x) { return x; }; }",
        "closure-shape-transition.js",
    )
    .unwrap();
    program.compile_released();
    let closure_id = program
        .functions
        .iter()
        .position(|function| {
            function.parent.is_some()
                && function
                    .name
                    .is_some_and(|atom| &program.atoms[atom as usize] == "plainName")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.initialize(&program).unwrap();
    vm.heap.retain_allocations_for_test();

    let first = vm.closure(&program, closure_id, Value::NULL).unwrap();
    let first_shape = vm.object_data(first).unwrap().shape();
    let shapes_after_first = vm.shapes.len();
    let mut roots = vec![vm.heap.root(first)];

    for _ in 1..CLOSURES {
        let closure = vm.closure(&program, closure_id, Value::NULL).unwrap();
        assert_eq!(vm.object_data(closure).unwrap().shape(), first_shape);
        roots.push(vm.heap.root(closure));
    }

    assert_eq!(vm.shapes.len(), shapes_after_first);
    assert_eq!(roots.len(), CLOSURES);
}

#[test]
fn closures_share_on_demand_function_source_and_program_roots_it() {
    let program = Engine::specialize(
        "function outer() { return function inner() { return 1; }; }",
        "closure-source-cache.js",
    )
    .unwrap();
    program.compile_released();
    let inner = program
        .functions
        .iter()
        .position(|function| {
            function.parent.is_some()
                && function
                    .name
                    .is_some_and(|atom| &program.atoms[atom as usize] == "inner")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.initialize(&program).unwrap();

    let first = vm.closure(&program, inner, Value::NULL).unwrap();
    let source_atom = vm.intern_atom("\0quench:function-source");
    // Creating a closure records no source; the text is made on first request.
    assert_eq!(vm.own_property(first, source_atom), None);
    let source = vm.function_source_value(first).unwrap();
    assert!(vm.heap.string(source).is_some_and(|text| text.host_string() == "function inner() { return 1; }"
    ));
    let source_weak = vm.heap.weak_handle(source).unwrap();
    let second = vm.closure(&program, inner, Value::NULL).unwrap();

    assert_eq!(vm.function_source_value(second), Some(source));
    vm.collect_now(&program);
    assert_eq!(vm.heap.weak_value(source_weak), Some(source));
}

#[test]
fn accessor_descriptor_transitions_do_not_root_values_in_the_cache() {
    use super::property_key::PropertyKey;

    let program = Engine::specialize("", "accessor-transition.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.initialize(&program).unwrap();
    vm.heap.retain_allocations_for_test();

    let atom = vm.intern_atom("field");
    let first_getter = vm.native_value(Native::Object);
    let second_getter = vm.native_value(Native::Array);
    let setter = vm.native_value(Native::Function);
    // A lone getter lives in its object's slot, so the two objects share one shape; a getter
    // with a setter lives in the shape's accessor table, so each object keeps its own.
    for (setter, shared) in [(None, true), (Some(setter), false)] {
        let first = vm.object();
        let second = vm.object();
        for (object, getter) in [(first, first_getter), (second, second_getter)] {
            vm.set_property(object, atom, Value::UNDEFINED).unwrap();
            vm.set_property_attributes(
                object,
                PropertyKey::string(atom),
                super::PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter,
                },
            );
        }

        let first_attributes = vm
            .property_attributes(first, PropertyKey::string(atom))
            .unwrap();
        let second_attributes = vm
            .property_attributes(second, PropertyKey::string(atom))
            .unwrap();
        assert_eq!(first_attributes.getter, Some(first_getter));
        assert_eq!(second_attributes.getter, Some(second_getter));
        assert_eq!((first_attributes.setter, second_attributes.setter), (setter, setter));
        assert_eq!(
            vm.object_data(first).unwrap().shape() == vm.object_data(second).unwrap().shape(),
            shared
        );
    }
}

#[test]
fn realm_lexical_state_follows_the_active_global_and_stays_rooted() {
    let mut vm = Vm::new(TestHost::default());
    let first_global = vm.object();
    let second_global = vm.object();
    vm.realm.globals = first_global;
    let first_binding = vm.intern_atom("firstRealmBinding");
    vm.realm.global_lexical_declarations.insert(first_binding);
    vm.realm
        .global_lexical_bindings
        .insert(first_binding, Value::number(1.0));

    assert_eq!(vm.switch_realm_global(second_global), first_global);
    assert!(
        !vm.realm
            .global_lexical_declarations
            .contains(&first_binding)
    );
    let second_binding = vm.intern_atom("secondRealmBinding");
    vm.realm.global_lexical_declarations.insert(second_binding);
    vm.realm
        .global_lexical_bindings
        .insert(second_binding, Value::number(2.0));

    let program = Engine::specialize("print(0);", "realm-switch.js").unwrap();
    vm.collect_now(&program);
    assert!(vm.heap.is_live(first_global));
    assert!(vm.heap.is_live(second_global));

    assert_eq!(vm.switch_realm_global(first_global), second_global);
    assert!(
        vm.realm
            .global_lexical_declarations
            .contains(&first_binding)
    );
    assert_eq!(
        vm.realm.global_lexical_bindings.get(&first_binding),
        Some(&Value::number(1.0))
    );
    assert!(
        !vm.realm
            .global_lexical_declarations
            .contains(&second_binding)
    );

    vm.switch_realm_global(second_global);
    assert!(
        vm.realm
            .global_lexical_declarations
            .contains(&second_binding)
    );
    assert_eq!(
        vm.realm.global_lexical_bindings.get(&second_binding),
        Some(&Value::number(2.0))
    );
}

#[test]
fn realm_intrinsic_registries_keep_each_realm_rooted() {
    let mut vm = Vm::new(TestHost::default());
    let first_global = vm.object();
    let second_global = vm.object();
    let first_error_prototype = vm.object();
    let second_error_prototype = vm.object();
    let first_iterator_prototype = vm.object();
    let second_iterator_prototype = vm.object();
    let first_regexp_constructor = vm.object();
    let first_regexp_prototype = vm.object();
    let second_regexp_constructor = vm.object();
    let second_regexp_prototype = vm.object();
    let first_segmenter_prototype = vm.object();
    let second_segmenter_prototype = vm.object();
    vm.realm
        .intrinsics
        .builtin_prototypes
        .insert((first_global, Native::TypeError), first_error_prototype);
    vm.realm
        .intrinsics
        .builtin_prototypes
        .insert((second_global, Native::TypeError), second_error_prototype);
    vm.realm.intrinsics.iterator_prototypes.insert(
        first_global,
        IteratorRealmPrototypes {
            helper: first_iterator_prototype,
            wrapper: first_iterator_prototype,
            generator: first_iterator_prototype,
            async_generator: first_iterator_prototype,
        },
    );
    vm.realm.intrinsics.regexp_intrinsics.insert(
        first_global,
        RegExpIntrinsics::new(first_regexp_constructor, first_regexp_prototype),
    );
    vm.realm.intrinsics.regexp_intrinsics.insert(
        second_global,
        RegExpIntrinsics::new(second_regexp_constructor, second_regexp_prototype),
    );
    vm.realm
        .intrinsics
        .intl_segmenter_prototypes
        .insert(first_global, first_segmenter_prototype);
    vm.realm
        .intrinsics
        .intl_segmenter_prototypes
        .insert(second_global, second_segmenter_prototype);
    vm.realm.intrinsics.iterator_prototypes.insert(
        second_global,
        IteratorRealmPrototypes {
            helper: second_iterator_prototype,
            wrapper: second_iterator_prototype,
            generator: second_iterator_prototype,
            async_generator: second_iterator_prototype,
        },
    );

    let program = Engine::specialize("print(0);", "realm-intrinsics.js").unwrap();
    vm.collect_now(&program);

    for object in [
        first_global,
        second_global,
        first_error_prototype,
        second_error_prototype,
        first_iterator_prototype,
        second_iterator_prototype,
        first_regexp_constructor,
        first_regexp_prototype,
        second_regexp_constructor,
        second_regexp_prototype,
        first_segmenter_prototype,
        second_segmenter_prototype,
    ] {
        assert!(vm.heap.is_live(object));
    }
    assert_eq!(
        vm.realm
            .intrinsics
            .builtin_prototypes
            .get(&(first_global, Native::TypeError)),
        Some(&first_error_prototype)
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .builtin_prototypes
            .get(&(second_global, Native::TypeError)),
        Some(&second_error_prototype)
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .iterator_prototypes
            .get(&first_global)
            .map(|prototypes| prototypes.generator),
        Some(first_iterator_prototype)
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .iterator_prototypes
            .get(&second_global)
            .map(|prototypes| prototypes.generator),
        Some(second_iterator_prototype)
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .regexp_intrinsics
            .get(&first_global)
            .map(|intrinsics| (intrinsics.constructor, intrinsics.prototype)),
        Some((first_regexp_constructor, first_regexp_prototype))
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .regexp_intrinsics
            .get(&second_global)
            .map(|intrinsics| (intrinsics.constructor, intrinsics.prototype)),
        Some((second_regexp_constructor, second_regexp_prototype))
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .intl_segmenter_prototypes
            .get(&first_global),
        Some(&first_segmenter_prototype)
    );
    assert_eq!(
        vm.realm
            .intrinsics
            .intl_segmenter_prototypes
            .get(&second_global),
        Some(&second_segmenter_prototype)
    );
}

#[test]
fn unreachable_promise_callback_and_aggregate_cycles_are_collected() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize(
        // A species other than %Promise% keeps a real capability; %Promise% reactions settle
        // their promise directly.
        "function make() { var p = new Promise(() => {}), q = new Promise(() => {}), value = {}; q.constructor = { [Symbol.species]: function (executor) { return new Promise(executor); } }; var handler = () => value; return [p, q, p.then(handler), q.then(handler), p.finally(handler), Promise.all([p])]; }",
        "promise-owner-cycles.js",
    ).unwrap();
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("make");
    let factory = vm.own_property(vm.realm.globals, name).unwrap();
    let values = vm
        .call_value(&program, factory, Value::UNDEFINED, &[])
        .unwrap();
    let promises: Vec<_> = vm
        .heap
        .array_elements(values)
        .unwrap()
        .iter()
        .copied()
        .filter(|value| vm.is_promise(*value))
        .collect();
    assert_eq!(promises.len(), 6);
    let owners: Vec<_> = promises
        .iter()
        .chain(vm.realm.promise.finally_handler_callbacks.keys())
        .chain(vm.realm.promise.aggregates.keys())
        .chain(vm.realm.promise.aggregate_jobs.keys())
        .chain(vm.realm.promise.reaction_capabilities.keys())
        .copied()
        .collect();
    assert!(!vm.realm.promise.finally_handler_callbacks.is_empty());
    assert!(!vm.realm.promise.aggregates.is_empty());
    assert!(!vm.realm.promise.aggregate_jobs.is_empty());
    assert!(!vm.realm.promise.reaction_capabilities.is_empty());
    let weak: Vec<_> = owners
        .iter()
        .map(|owner| vm.heap.weak_handle(*owner).unwrap())
        .collect();
    let root = vm.heap.root(values);
    vm.collect_now(&program);
    assert!(
        weak.iter()
            .all(|handle| vm.heap.weak_value(*handle).is_some())
    );
    assert!(vm.heap.release_root(root));
    vm.collect_now(&program);
    assert!(
        weak.iter()
            .all(|handle| vm.heap.weak_value(*handle).is_none())
    );
    assert!(vm.realm.promise.finally_handler_callbacks.is_empty());
    assert!(vm.realm.promise.aggregates.is_empty());
    assert!(vm.realm.promise.aggregate_jobs.is_empty());
    assert!(vm.realm.promise.reaction_capabilities.is_empty());
}

#[test]
fn loop_clones_reset_bindings_of_scopes_out_of_reach() {
    let program = Engine::specialize(
        "function f() { var g; for (let i = 0; i < 2; i++) { for (let k = 0; k < 2; k++) { g = () => k; } g = () => i; } return g; }",
        "loop-clone-reset.js",
    )
    .unwrap();
    program.compile_released();
    let plans: Vec<_> = program
        .functions
        .iter()
        .flat_map(|function| function.environment_clones.iter())
        .collect();
    // The outer loop's next iteration does not share the inner loop's `k`
    // with the clone that last owned it: it resets the slot.
    assert!(plans.iter().any(|plan| !plan.reset.is_empty()));
    assert!(plans.iter().all(|plan| {
        plan.reset
            .iter()
            .all(|slot| plan.fresh.binary_search(slot).is_err())
    }));
}

#[test]
fn loop_iterations_release_their_environments_and_eval_programs() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize(
        "var kept = []; function run(n) { for (let i = 0; i < n; i++) { let m; eval('m = (() => { const x = ' + i + '; return () => x; })()'); for (let k = 0; k < 2; k++) {} kept.push(m()); } }",
        "loop-eval-reclaim.js",
    )
    .unwrap();
    vm.execute(&program).unwrap();
    let programs_before = vm.programs.live_count();
    let name = vm.intern_atom("run");
    let run = vm.own_property(vm.realm.globals, name).unwrap();
    vm.call_value(&program, run, Value::UNDEFINED, &[Value::number(4.0)])
        .unwrap();
    assert!(vm.programs.live_count() > programs_before);
    // A program survives the first reclamation pass after its insertion.
    vm.collect_now(&program);
    vm.collect_now(&program);
    assert_eq!(vm.programs.live_count(), programs_before);
    let name = vm.intern_atom("kept");
    let kept = vm.own_property(vm.realm.globals, name).unwrap();
    let joined = vm.to_string(&program, kept).unwrap();
    assert_eq!(&*joined, "0,1,2,3");
}

#[test]
fn property_sites_past_the_site_space_share_a_site_per_name() {
    // Two passes over a line of sites larger than the 16-bit site space: the reads past the
    // unique sites share one site per name, alternately `a` and `b` of the same shape.
    let sites = usize::from(u16::MAX) + 64;
    let reads = (0..sites)
        .map(|index| {
            if index % 2 == 0 {
                "s += o.a;"
            } else {
                "s += o.b;"
            }
        })
        .collect::<String>();
    let source = format!(
        "var o = {{ a: 1, b: 2 }}; function sum() {{ var s = 0; {reads} return s; }} var total = sum() + sum();"
    );
    let program = Engine::specialize(&source, "site-overflow.js").unwrap();
    assert_eq!(
        program.cache_sites,
        crate::bytecode::SHARED_UNCACHED_SITE + 1
    );
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("total");
    let total = vm.own_property(vm.realm.globals, name).unwrap();
    let expected = 2 * (sites / 2 * 3 + sites % 2);
    assert_eq!(total.as_number(), Some(expected as f64));
}

#[test]
fn super_inside_with_reads_the_home_object() {
    // A `with` object cannot shadow the compiler's binding of a method's home object.
    let source = "var o = { m() { with ({ x: 9 }) { return super.x + x + (() => super.x)(); } } };\n\
                  Object.setPrototypeOf(o, { x: 3 });\n\
                  var result = o.m();";
    let program = Engine::specialize(source, "super-inside-with.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("result");
    let result = vm.own_property(vm.realm.globals, name).unwrap();
    assert_eq!(result.as_number(), Some(15.0));
}

#[test]
fn name_shared_sites_keep_loads_apart_from_stores() {
    // Past the unique sites, a load of `a` from a read-only field and an inherited getter
    // records entries a store of `a` must not take: the store fails or runs the setter.
    let filler = (0..usize::from(crate::bytecode::UNIQUE_CACHE_SITES))
        .map(|_| "o.z;")
        .collect::<String>();
    let source = format!(
        "var o = {{ z: 0 }}; function filler() {{ {filler} }}\n\
         var ro = Object.defineProperty({{}}, 'a', {{ value: 5, writable: false }});\n\
         var calls = 0; var proto = {{ get a() {{ return 7; }}, set a(v) {{ calls += v; }} }};\n\
         var child = Object.create(proto);\n\
         function read(x) {{ return x.a; }} function write(x, v) {{ x.a = v; }}\n\
         var seen = '';\n\
         for (var i = 0; i < 4; i++) {{ seen += read(ro) + ',' + read(child) + ';'; write(ro, 9); write(child, 1); }}\n\
         var result = seen + calls + ':' + ro.a + ':' + Object.keys(child).length;"
    );
    let program = Engine::specialize(&source, "name-shared-sites.js").unwrap();
    assert_eq!(program.cache_sites, crate::bytecode::SHARED_UNCACHED_SITE + 1);
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("result");
    let result = vm.own_property(vm.realm.globals, name).unwrap();
    assert_eq!(&*vm.to_string(&program, result).unwrap(), "5,7;5,7;5,7;5,7;4:5:0");
}

#[test]
fn for_of_iterator_cleanup_survives_body_temporaries() {
    // Closing the outer generator while its loop body is suspended in `yield*` closes the
    // loop's iterator; the body's temporaries must not have reused the iterator's register.
    let program = Engine::specialize(
        "function inner(x) { return { *[Symbol.iterator]() { yield x; } }; }\n\
         var closed = 0;\n\
         var source = { [Symbol.iterator]() { var i = 0; return { next() { return { value: ++i, done: i > 3 }; }, return() { closed++; return {}; } }; } };\n\
         function flat(s) { return { *[Symbol.iterator]() { for (let d of source) { if (s) { yield d; } else { yield* inner(d); } } } }; }\n\
         function* take(data) { for (let d of data) { yield d; return; } }\n\
         var taken = [...take(flat(false))].join() + ':' + closed;",
        "for-of-cleanup.js",
    )
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("taken");
    let taken = vm.own_property(vm.realm.globals, name).unwrap();
    assert_eq!(&*vm.to_string(&program, taken).unwrap(), "1:1");
}

fn global_text(source: &str, name: &str) -> String {
    global_text_under(crate::MemoryPolicy::default(), source, name)
}

fn global_text_under(policy: crate::MemoryPolicy, source: &str, name: &str) -> String {
    let program = Engine::specialize(source, "global-text.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_memory_policy(policy);
    vm.execute(&program).unwrap();
    let name = vm.intern_atom(name);
    let value = vm.own_property(vm.realm.globals, name).unwrap();
    vm.to_string(&program, value).unwrap().to_string()
}

#[test]
fn legacy_date_strings_parse_like_node() {
    // Node's (V8's) legacy fallback: named months, slashed dates, AM/PM, zone names,
    // GMT offsets, comments and 24:00; garbage words after numbers reject the string.
    let cases = [
        ("Aug 4 2011 10:00 PM GMT", "1312495200000"),
        ("4 August 2011 10:00 GMT", "1312452000000"),
        ("8/4/2011 10:00:30.5 PST", "1312480830500"),
        ("Tue Oct 14 2026 00:00:00 GMT+0530 (India)", "1791916200000"),
        ("Aug 4 2011 10:00 GMT+5", "1312434000000"),
        ("2011/08/04 24:00 UTC", "1312502400000"),
        ("Aug 4 2011 13:00 PM GMT", "NaN"),
        ("foo2011 GMT", "NaN"),
        ("Aug 4 2011 10:00x GMT", "NaN"),
    ];
    let source = cases
        .iter()
        .map(|(text, _)| format!("Date.parse({text:?})"))
        .collect::<Vec<_>>()
        .join(", ");
    let expected = cases.map(|(_, millis)| millis).join(",");
    assert_eq!(
        global_text(&format!("var parsed = [{source}].join();"), "parsed"),
        expected
    );
}

#[test]
fn reused_frames_resolve_captures_from_their_own_environment_state() {
    // `worker` owns an environment (captured), so its depth-0 captures are its parent's. A
    // later frame running `lookup` sees that same environment as its parent, where depth 0
    // is the environment itself; a cached resolution must not carry across that change.
    let source = "'use strict';\n\
         var result = (() => { var t = 0, i = 'wrong';\n\
           function trace() {}\n\
           function worker(first, t, n) {\n\
             if (first) return lookup();\n\
             return trace(), lookup();\n\
             function lookup() { return n; }\n\
           }\n\
           return [worker(true, 0, 'own'), worker(false, 0, 'own')].join();\n\
         })();";
    assert_eq!(global_text(source, "result"), "own,own");
}

#[test]
fn collection_iterators_follow_deletes_compaction_and_clear() {
    // Deletes leave holes and compaction renumbers registered iterators; a `forEach` walk
    // defers compaction and still visits entries added after a `clear`. Node agrees.
    let source = "var out = [];\n\
         var m = new Map(); for (var i = 0; i < 100; i++) m.set(i, i);\n\
         var it = m.keys(); for (var i = 0; i < 10; i++) it.next();\n\
         for (var i = 0; i < 90; i++) m.delete(i);\n\
         out.push(it.next().value, m.size, m.get(95), m.has(5));\n\
         var walked = []; var w = new Map(); for (var i = 0; i < 50; i++) w.set(i, i);\n\
         w.forEach(function (v, k) { walked.push(k);\n\
           if (k === 0) { for (var j = 1; j < 45; j++) w.delete(j); w.set('late', 1); }\n\
           if (k === 'late') { w.clear(); w.set('after', 2); } });\n\
         out.push(walked.join(' '), w.size);\n\
         var s = new Set([0, -0, NaN, NaN, 'k', 'k']); out.push(s.size, s.has(-0), s.has(NaN));\n\
         var result = out.join();";
    assert_eq!(
        global_text(source, "result"),
        "90,10,95,false,0 45 46 47 48 49 late after,1,3,true,true"
    );
}

#[test]
fn inner_lexical_bindings_shadow_outer_immutable_captures() {
    let source = "'use strict';\n\
         const a = 1;\n\
         var result = (() => { let a = []; [1, 2].forEach(e => { a = a.concat(e); }); return a.join(); })();";
    assert_eq!(global_text(source, "result"), "1,2");
}

#[test]
fn local_bindings_shadow_captured_function_names_and_constants() {
    // A parameter, var or let inside a nested function shadows the enclosing function
    // expression's name, and a var in a finally block is a function-scoped local.
    let source = "'use strict';\n\
         const n = 0;\n\
         const named = function e() {\n\
           function param(e) { e = 2; return e; }\n\
           function local() { var e; e = 3; return e; }\n\
           function block() { let e; e = 4; return e; }\n\
           return [param(1), local(), block()].join();\n\
         };\n\
         function finallyVar() { try { return 5; } finally { if (true) { var n = 6; } } }\n\
         var result = named() + ',' + finallyVar();\n\
         var message = (() => { try { n = 1; } catch (error) { return error.message; } })();";
    assert_eq!(global_text(source, "result"), "2,3,4,5");
    assert_eq!(global_text(source, "message"), "Assignment to constant variable.");
}

#[test]
fn array_results_survive_collections_in_proxy_traps() {
    // splice, slice and concat build their result while proxy traps run guest code
    // that allocates enough to collect.
    let source = "var churn = () => { let keep; for (let i = 0; i < 20000; i++) keep = { i, s: 'x' + i }; return keep; };\n\
         var target = []; for (let i = 0; i < 64; i++) target.push({ v: i });\n\
         var proxy = new Proxy(target, { get(t, k, r) { churn(); return Reflect.get(t, k, r); } });\n\
         var spliced = Array.prototype.splice.call(proxy, 0, 32);\n\
         var sliced = Array.prototype.slice.call(proxy, 0, 16);\n\
         var joined = Array.prototype.concat.call([], proxy, proxy).length;\n\
         var result = [spliced.length, spliced[31].v, sliced[15].v, joined, target.length].join();";
    assert_eq!(global_text(source, "result"), "32,31,47,64,32");
}

#[test]
fn private_names_resolve_per_class_across_collections() {
    // Each class evaluation has its own private names; resolution is memoized per
    // environment and must still reject foreign objects across collections.
    let source = "function make() { return class { #x = 1; get() { return this.#x; } has(o) { return #x in o; } read(o) { return o.#x; } }; }\n\
         var A = make(), B = make(), a = new A(), b = new B(), out = [];\n\
         for (let round = 0; round < 3; round++) { out.push(a.get(), b.get(), a.has(b), a.has(a)); for (let i = 0; i < 50000; i++) ({ i }); }\n\
         try { a.read(b); } catch (error) { out.push(error.constructor.name); }\n\
         var result = out.join();";
    assert_eq!(
        global_text(source, "result"),
        "1,1,false,true,1,1,false,true,1,1,false,true,TypeError"
    );
}

#[test]
fn json_and_object_keys_read_enumerable_keys_in_own_property_order() {
    // Shape-backed objects skip descriptor objects; order, enumerability, wrapper unboxing
    // and the keys handed to toJSON and replacers stay those of the generic path.
    let source = "var o = { b: 1, 2: 'x', a: 2, 1: 'y', '01': 3, 4294967295: 4 };\n\
         Object.defineProperty(o, 'h', { value: 0, enumerable: false });\n\
         Object.defineProperty(o, 'g', { get() { delete this.z; return 5; }, enumerable: true });\n\
         o.z = 6; delete o.b; o.b = 7;\n\
         var seen = [];\n\
         var replaced = JSON.stringify({ a: [8, { toJSON(k) { return typeof k + k; } }] }, function (k, v) { seen.push(k); return v; });\n\
         var p = new Proxy({ a: 1, b: 2 }, { ownKeys: () => ['b', 'a'], getOwnPropertyDescriptor: (t, k) => ({ value: t[k], enumerable: k !== 'a', configurable: true }) });\n\
         var result = [JSON.stringify(o), Object.keys(o).join(), replaced, seen.join(), JSON.stringify(p), Object.keys(p).join(),\n\
           JSON.stringify([new Number(1), new String('s\"'), new Boolean(false)]), JSON.stringify(o, ['b', 1, 'b'])].join('|');";
    assert_eq!(
        global_text(source, "result"),
        "{\"1\":\"y\",\"2\":\"x\",\"a\":2,\"01\":3,\"4294967295\":4,\"g\":5,\"b\":7}|1,2,a,01,4294967295,g,b|\
         {\"a\":[8,\"string1\"]}|,a,0,1|{\"b\":2}|b|[1,\"s\\\"\",false]|{\"b\":7,\"1\":\"y\"}"
    );
}

#[test]
fn string_indices_are_canonical_and_iterator_results_define_data_properties() {
    // Only canonical array-index names read a string's units; other names reach the
    // prototype. Iterator results define `value` and `done` without running setters.
    let source = "String.prototype[3] = 'p';\n\
         var s = 'abc', keys = ['01', '+1', '1', '1.0', '3', 2];\n\
         var units = keys.map(function (k) { return String(s[k]); }).join();\n\
         Object.defineProperty(Object.prototype, 'value', { set: function () { throw 1; }, configurable: true });\n\
         var it = (function* () { yield 1; })(), first = it.next(), last = it.next();\n\
         delete Object.prototype.value; delete String.prototype[3];\n\
         var result = [units, Object.keys(first).join(), first.value, first.done, last.done].join('|');";
    assert_eq!(
        global_text(source, "result"),
        "undefined,undefined,b,undefined,p,c|value,done|1|false|true"
    );
}

#[test]
fn spread_accumulates_without_user_methods_or_prototype_setters() {
    // ArrayAccumulation defines elements at the next index: elisions advance it, and neither
    // `push`, `apply`, `Reflect.construct` nor an inherited index setter is consulted.
    let source = "var push = Array.prototype.push, apply = Function.prototype.apply, construct = Reflect.construct, seen = 0;\n\
         Array.prototype.push = Function.prototype.apply = Reflect.construct = function () { seen++; };\n\
         Object.defineProperty(Array.prototype, 1, { set: function () { seen++; }, configurable: true });\n\
         function f() { return arguments.length; }\n\
         function C() { this.n = arguments.length; }\n\
         var a = [, ...[1], , 2, ,], b = [...'xy', ...[ , 3]];\n\
         var calls = [f(...[1, 2], 3), new C(0, ...[1, 2]).n];\n\
         Array.prototype.push = push; Function.prototype.apply = apply; Reflect.construct = construct; delete Array.prototype[1];\n\
         var result = [a.length, Object.keys(a).join(), b.join(), Object.keys(b).join(), calls.join(), seen].join('|');";
    assert_eq!(global_text(source, "result"), "5|1,3|x,y,,3|0,1,2,3|3,3|0");
}

#[test]
fn push_and_splice_growth_reaches_inherited_setters_and_proxy_traps() {
    // Indices that `push` and `splice` grow into are [[Set]] on the receiver: a proxy or an
    // index setter on the prototype chain answers them however the setter was defined, and a
    // setter later replaced by data no longer does.
    let source = "var seen = [];\n\
         var proxy = new Proxy(Object.create(Array.prototype), { set: function (t, k, v) { seen.push('trap' + k + v); return true; } });\n\
         var literal = Object.setPrototypeOf({ set 2(v) { seen.push('literal' + v); } }, Array.prototype);\n\
         var legacy = Object.create(Array.prototype); legacy.__defineSetter__('1', function (v) { seen.push('legacy' + v); });\n\
         var replaced = Object.create(Array.prototype, { 1: { set: function () { seen.push('never'); }, configurable: true } });\n\
         Object.defineProperty(replaced, 1, { value: 0, writable: true, configurable: true });\n\
         function on(proto) { var a = [0]; Object.setPrototypeOf(a, proto); return a; }\n\
         var a = on(proxy), b = on(proxy), c = on(literal), d = on(legacy), e = on(replaced);\n\
         a.push('p'); b.splice(1, 0, 's'); c.push('x', 'y'); d.splice(0, 1, 'u', 'v'); e.push('w');\n\
         var result = [seen.join(), Object.keys(a).join(), Object.keys(b).join(), Object.keys(c).join(), Object.keys(d).join(), e.join()].join('|');";
    assert_eq!(
        global_text(source, "result"),
        "trap1p,trap1s,literaly,legacyv|0|0|0,1|0|0,w"
    );
}

#[test]
fn array_index_stores_respect_inherited_read_only_properties() {
    // OrdinarySet: an index the receiver lacks fails when a prototype holds it read-only,
    // whether defined so, frozen (array or object) or a String wrapper's character, and
    // `push`/`splice` throw. A writable inherited element is shadowed by an own one.
    let source = "var out = [];\n\
         var readOnly = Object.create(Array.prototype); Object.defineProperty(readOnly, 1, { value: 'r' });\n\
         var protos = [readOnly, Object.setPrototypeOf(Object.freeze(['a', 'b']), Array.prototype), Object.freeze(Object.setPrototypeOf({ 1: 'o' }, Array.prototype)), Object.setPrototypeOf(new String('ab'), Array.prototype), Object.setPrototypeOf({ 1: 'w' }, Array.prototype)];\n\
         var ops = [function (a) { 'use strict'; a[1] = 'v'; }, function (a) { 'use strict'; Object.create(a)['1'] = 'v'; }, function (a) { a[1] = 'v'; }, function (a) { a['1'] = 'v'; }, function (a) { a.push('v'); }, function (a) { a.splice(1, 0, 'v'); }, function (a) { Object.create(a)[1] = 'v'; }];\n\
         protos.forEach(function (proto) {\n\
           out.push(ops.map(function (op) {\n\
             var a = [0]; Object.setPrototypeOf(a, proto);\n\
             try { (function () { 'use strict'; op(a); })(); return Object.keys(a).join(''); } catch (e) { return e.name[0]; }\n\
           }).join(','));\n\
           var sloppy = [0]; Object.setPrototypeOf(sloppy, proto); sloppy[1] = 'v';\n\
           out.push(Object.keys(sloppy).join(''));\n\
         });\n\
         var result = out.join('|');";
    assert_eq!(
        global_text(source, "result"),
        "T,T,0,0,T,T,0|0|T,T,0,0,T,T,0|0|T,T,0,0,T,T,0|0|T,T,0,0,T,T,0|0|01,0,01,01,01,01,0|01"
    );
}

#[test]
fn typed_array_index_stores_convert_and_ignore_out_of_range_indices() {
    // Integer-indexed stores go straight to TypedArraySetElement: values convert by kind,
    // out-of-range indices and prototype setters are never reached, and a non-typed
    // receiver inheriting from a typed array gets an ordinary own property.
    let source = "var u = new Uint8Array(4), c = new Uint8ClampedArray(1), seen = 0;\n\
         Object.defineProperty(Object.getPrototypeOf(Uint8Array.prototype), 5, { set: function () { seen++; }, configurable: true });\n\
         u[0] = 300; u[1] = -1; u[2] = { valueOf: function () { return 7; } }; u[5] = 1; c[0] = 300;\n\
         var o = Object.create(u); o[0] = 42;\n\
         delete Object.getPrototypeOf(Uint8Array.prototype)[5];\n\
         var result = [Array.from(u).join(), c[0], seen, u[5], u[0], Object.keys(o).join()].join('|');";
    assert_eq!(global_text(source, "result"), "44,255,7,0|255|0||44|0");
}

#[test]
fn finished_jobs_release_their_values_while_the_queue_keeps_draining() {
    // A job leaves the queue before it runs, so a value only its arguments held is
    // collectable while later jobs of the same drain still run: its cleanup is observed
    // inside the await loop instead of never. Each iteration allocates at least one cell,
    // so the bound lets the drain pass the collection headroom several times. Memory-first
    // pacing collects soonest, which keeps the drain short.
    let policy = crate::MemoryPolicy::MemoryFirst;
    let iterations = 4 * crate::heap::full_collection_allocation_bound(policy);
    let source = format!(
        "var finalized = false, registry = new FinalizationRegistry(function () {{ finalized = true; }});\n\
         function make() {{ var o = {{}}; registry.register(o, 0); return o; }}\n\
         var result = 'pending';\n\
         (async function () {{\n\
           await Promise.resolve(make());\n\
           for (var i = 0; i < {iterations} && !finalized; i++) await {{ i: i }};\n\
           result = String(finalized);\n\
         }})();"
    );
    assert_eq!(global_text_under(policy, &source, "result"), "true");
}

#[test]
fn json_parse_builds_the_same_values_with_and_without_a_reviver() {
    // Without a reviver values are built during the parse; with one, from the source tree.
    // Both give escapes, numbers, duplicate keys, key order and `__proto__` the same way.
    let source = "var text = '{\"b\":\"x\\\\ny\\\\u00e9\",\"2\":[1,-0,2.5e3,1E-2],\"a\":{\"__proto__\":{\"k\":1}},\"b\":true,\"\":null}';\n\
         var plain = JSON.parse(text), revived = JSON.parse(text, function (k, v) { return v; });\n\
         var describe = function (o) { return [Object.keys(o).join(), JSON.stringify(o), Object.is(o['2'][1], -0), Object.getPrototypeOf(o.a) === Object.prototype, Object.keys(o.a).join()].join('|'); };\n\
         var result = describe(plain) === describe(revived) ? describe(plain) : 'differs: ' + describe(plain) + ' / ' + describe(revived);";
    assert_eq!(
        global_text(source, "result"),
        "2,b,a,|{\"2\":[1,0,2500,0.01],\"b\":true,\"a\":{\"__proto__\":{\"k\":1}},\"\":null}|true|true|__proto__"
    );
}

#[test]
fn promise_record_edges_follow_promise_and_resolver_lifetime() {
    let mut vm = Vm::new(TestHost::default());
    let promise = vm.promise_object().unwrap();
    let result = vm.object();
    let program = Engine::specialize("print(0);", "realm-promises.js").unwrap();
    let owner_atom = vm.intern_atom("owner");
    vm.set_property(result, owner_atom, promise).unwrap();
    vm.promise_settle(
        &program,
        promise,
        super::promise::PromiseState::Fulfilled,
        result,
    )
    .unwrap();
    let promise_weak = vm.heap.weak_handle(promise).unwrap();
    let result_weak = vm.heap.weak_handle(result).unwrap();
    let promise_root = vm.heap.root(promise);
    vm.collect_now(&program);

    assert!(vm.heap.is_live(promise));
    assert!(vm.heap.is_live(result));
    assert_eq!(
        vm.promise_record(promise)
            .map(|record| record.result),
        Some(result)
    );
    let (resolve, _) = vm.promise_resolving_functions(promise);
    let resolver_root = vm.heap.root(resolve);
    assert!(vm.heap.release_root(promise_root));
    vm.collect_now(&program);
    assert_eq!(vm.heap.weak_value(promise_weak), Some(promise));
    assert_eq!(vm.heap.weak_value(result_weak), Some(result));
    assert!(vm.heap.release_root(resolver_root));
    vm.collect_now(&program);
    assert!(vm.heap.weak_value(promise_weak).is_none());
    assert!(vm.heap.weak_value(result_weak).is_none());
    assert!(!vm.heap.is_live(promise));
}

#[test]
fn js_error_is_pointer_sized() {
    assert_eq!(size_of::<JsError>(), size_of::<usize>());
}

#[test]
fn dynamic_primitive_strings_are_canonicalized() {
    let mut vm = Vm::new(TestHost::default());
    let first = vm.intern_dynamic_value("same text".into());
    let second = vm.intern_dynamic_value("same text".into());
    assert_eq!(first, second);
}

#[test]
fn primitive_strings_keep_the_intrinsic_prototype_after_global_reassignment() {
    let program = Engine::specialize(
        r#"
            function readCode() { return "a".charCodeAt(0); }
            if (readCode() !== 97) throw new Error("initial String prototype");
            String.prototype.charCodeAt = function() { return 99; };
            if (readCode() !== 99) throw new Error("live prototype value");
            String = function ReplacedString() {};
            if (readCode() !== 99) throw new Error("intrinsic String prototype");
        "#,
        "primitive-string-intrinsic-prototype.js",
    )
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
}

#[test]
fn repeated_string_concatenations_use_the_bounded_cache() {
    let mut vm = Vm::new(TestHost::default());
    let left = vm.intern_dynamic_value("left".into());
    let right = vm.intern_dynamic_value("right".into());
    let first = vm.intern_dynamic_concat(left, right).unwrap();
    let second = vm.intern_dynamic_concat(left, right).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        vm.string_concats.as_ref().unwrap().len(),
        super::STRING_CONCAT_CACHE_SIZE
    );
}

#[test]
fn dynamic_atoms_distinguish_lone_surrogate_units() {
    let mut vm = Vm::new(TestHost::default());
    let first = vm.intern_js_atom(&JsString::from_units(&[0xD800]));
    let second = vm.intern_js_atom(&JsString::from_units(&[0xD800]));
    let other = vm.intern_js_atom(&JsString::from_units(&[0xD801]));
    assert_eq!(first, second);
    assert_ne!(first, other);
    assert_eq!(vm.atom_value(first).units(), &[0xD800]);
}

#[test]
fn accessor_descriptors_share_get_and_set_property_semantics() {
    let source = r#"
      var object = {};
      Object.defineProperty(object, "value", {
        get: function() { return 3; },
        set: function(next) { print(next); }
      });
      print(object.value);
      object.value = 9;
      var descriptor = Object.getOwnPropertyDescriptor(object, "value");
      print(typeof descriptor.get);
      print(typeof descriptor.set);
      var prototype = {};
      var receiver = Object.create(prototype);
      Object.defineProperty(prototype, "inherited", {
        get: function() { return this === receiver ? 7 : 0; },
        set: function(next) { print(this === receiver); }
      });
      print(receiver.inherited);
      receiver.inherited = 11;
      var lockedPrototype = {};
      Object.defineProperty(lockedPrototype, "locked", { value: 1, writable: false });
      var lockedReceiver = Object.create(lockedPrototype);
      try { lockedReceiver.locked = 2; print("not-blocked"); }
      catch (error) { print("blocked"); }
      print(lockedReceiver.locked);
      print(Object.hasOwn(lockedReceiver, "locked"));
      class Accessor {
        constructor(value) { this._value = value; }
        get value() { return this._value; }
        set value(next) { this._value = next; }
        static get kind() { return "class"; }
      }
      var instance = new Accessor(4);
      print(instance.value);
      instance.value = 6;
      print(instance.value);
      print(Accessor.kind);
    "#;
    for strict in [false, true] {
        let source = if strict {
            format!("'use strict';\n{source}")
        } else {
            source.to_owned()
        };
        for compile in [Engine::specialize, Engine::specialize_unspecialized] {
            let program = compile(&source, "accessor.js").unwrap();
            let output = Rc::new(RefCell::new(Vec::new()));
            let mut vm = Vm::new(TestHost::writing_to(output.clone()));
            vm.execute(&program).unwrap();
            assert_eq!(
                output.borrow().as_slice(),
                [
                    "3",
                    "9",
                    "function",
                    "function",
                    "7",
                    "true",
                    if strict { "blocked" } else { "not-blocked" },
                    "1",
                    "false",
                    "4",
                    "6",
                    "class"
                ],
                "strict={strict}"
            );
        }
    }
}

#[test]
fn record_receivers_cache_virtual_names_and_inherited_fields_like_ordinary_cells() {
    // Objects built by `new` live in record spaces. Their `size`/`length` reads and fields
    // inherited two prototypes up must reach the same cache tiers as legacy ordinary cells,
    // while exotic receivers sharing a site keep their own `length`.
    let source = r#"
      function Collection() { this.elms = [1, 2, 3]; }
      Collection.prototype.size = function () { return this.elms.length; };
      function Base() {}
      Base.prototype.kind = "base";
      function Mid() {}
      Mid.prototype = new Base();
      function Leaf() {}
      Leaf.prototype = new Mid();
      function readSize(x) { return x.size; }
      function readLength(x) { return x.length; }
      function readKind(x) { return x.kind; }
      var collection = new Collection();
      var leaf = new Leaf();
      var holder = { length: "proto-length" };
      var ordinary = Object.create(holder);
      var array = [];
      Object.setPrototypeOf(array, holder);
      var text = new String("abcd");
      Object.setPrototypeOf(text, holder);
      for (var i = 0; i < 3; i++) {
        print(typeof readSize(collection) + " " + collection.size() + " " + readKind(leaf));
        print(readLength(ordinary) + " " + readLength(array) + " " + readLength(text));
      }
      Base.prototype.kind = "changed";
      print(readKind(leaf));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "record-virtual-names.js").unwrap();
    vm.execute(&program).unwrap();
    let mut expected = Vec::new();
    for _ in 0..3 {
        expected.push("function 3 base".to_owned());
        expected.push("proto-length 0 4".to_owned());
    }
    expected.push("changed".to_owned());
    assert_eq!(*output.borrow(), expected);
    assert!(
        vm.field_caches
            .iter()
            .any(|cache| cache.holder != NO_FIELD_HOLDER),
        "a prototype-held `size` read on a record receiver filled a holder entry"
    );
    assert!(
        !vm.inherited_field_caches.is_empty(),
        "a field two prototypes above a record receiver filled the inherited cache"
    );
}

#[test]
fn dictionary_shapes_fall_back_after_deletion_and_prototype_use() {
    let source = r#"
      var prototype = {
        answer: 42,
        getAnswer: function() { return this.answer; }
      };
      var inherited = {};
      var inheritor = Object.create(inherited);
      Object.setPrototypeOf(inherited, prototype);
      print(inherited.answer);
      print(inherited.getAnswer());
      var deleted = { answer: 5, other: 6 };
      delete deleted.answer;
      delete deleted.other;
      deleted.answer = 7;
      print(deleted.answer);
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = compile(source, "dictionary-shapes.js").unwrap();
        vm.execute(&program).unwrap();

        let global = vm.realm.globals;
        for name in ["inherited", "deleted"] {
            let atom = vm.intern_atom(name);
            let object = vm
                .own_property(global, atom)
                .expect("global binding exists");
            let shape = vm.object_data(object).expect("object value").shape();
            assert!(
                vm.field_caches.iter().all(|cache| cache.receiver != shape),
                "{mode}: dictionary shape entered a monomorphic field cache for {name}"
            );
            assert!(
                vm.megamorphic_fields
                    .iter()
                    .all(|cache| cache.get(shape, NO_FIELD_HOLDER).is_none()),
                "{mode}: dictionary shape entered a megamorphic field cache for {name}"
            );
            assert!(
                vm.method_caches
                    .iter()
                    .flatten()
                    .all(|cache| cache.shape != shape),
                "{mode}: dictionary shape entered a monomorphic method cache for {name}"
            );
            assert!(
                vm.megamorphic_methods.iter().all(|cache| {
                    cache.entries[..usize::from(cache.len)]
                        .iter()
                        .all(|entry| entry.shape != shape)
                }),
                "{mode}: dictionary shape entered a megamorphic method cache for {name}"
            );
        }

        for (name, trigger) in [
            ("inherited", super::DictionaryTrigger::PrototypeUse),
            ("deleted", super::DictionaryTrigger::DeletionPattern),
        ] {
            let atom = vm.intern_atom(name);
            let object = vm
                .own_property(global, atom)
                .expect("global binding exists");
            let shape = vm.object_data(object).expect("object value").shape();
            assert!(vm.shape_is_dictionary(shape), "{mode}: {name}");
            assert_eq!(
                vm.shapes[shape as usize].dictionary_trigger,
                Some(trigger),
                "{mode}: {name} trigger"
            );
        }

        vm.collect_now(&program);
        let atom = vm.intern_atom("inherited");
        let inherited = vm.own_property(global, atom).unwrap();
        let shape = vm.object_data(inherited).unwrap().shape();
        assert!(
            vm.shape_is_dictionary(shape),
            "{mode}: dictionary mode survives GC"
        );
        let atom = vm.intern_atom("deleted");
        let deleted = vm
            .own_property(global, atom)
            .expect("global binding exists");
        let atom = vm.intern_atom("answer");
        assert_eq!(
            vm.own_property(deleted, atom).and_then(Value::as_number),
            Some(7.0),
            "{mode}: dictionary-backed property survives GC"
        );
        assert_eq!(output.borrow().as_slice(), ["42", "42", "7"], "{mode}");
        #[cfg(feature = "profile-aggregate")]
        for trigger in [
            super::DictionaryTrigger::DeletionPattern,
            super::DictionaryTrigger::PrototypeUse,
        ] {
            assert!(vm.profile.dictionary_transitions[trigger.index()] > 0);
        }
    }
}

#[test]
fn prototype_change_on_a_non_prototype_keeps_shape_and_caches_sound() {
    // Two receivers share a shape while their prototypes differ and then swap: every
    // cached load, store, add and method call must follow each receiver's actual chain.
    let source = r#"
      function A() {} A.prototype.kind = "a"; A.prototype.who = function() { return "A" + this.n; };
      function B() {} B.prototype.kind = "b"; B.prototype.who = function() { return "B" + this.n; };
      Object.defineProperty(B.prototype, "late", { set(v) { this.seen = v; }, configurable: true });
      function read(o) { return o.kind + o.who() + o.n; }
      function add(o) { o.late = 1; return Object.prototype.hasOwnProperty.call(o, "late"); }
      var out = [];
      for (var i = 0; i < 3; i++) {
        var x = { n: i }, y = { n: i };
        Object.setPrototypeOf(x, A.prototype);
        Object.setPrototypeOf(y, B.prototype);
        out.push(read(x), read(y), add(x), add(y));
        Object.setPrototypeOf(x, B.prototype);
        Object.setPrototypeOf(y, A.prototype);
        out.push(read(x), read(y));
      }
      var array = [];
      Object.setPrototypeOf(array, { length: "inherited" });
      out.push(read2(array));
      print(out.join(","));
      function read2(o) { return o.length; }
    "#;
    let expected = (0..3)
        .map(|i| format!("aA{i}{i},bB{i}{i},true,false,bB{i}{i},aA{i}{i}"))
        .chain(["0".to_owned()])
        .collect::<Vec<_>>()
        .join(",");
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = compile(source, "prototype-change.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), [expected.as_str()], "{mode}");
        let atom = vm.intern_atom("x");
        let x = vm.own_property(vm.realm.globals, atom).unwrap();
        let shape = vm.object_data(x).unwrap().shape();
        assert!(!vm.shape_is_dictionary(shape), "{mode}: receiver keeps a cacheable shape");
    }
}

#[test]
fn moved_shape_lookup_index_matches_the_derived_index() {
    // A wide object built key by key, with non-default attributes and redefinitions, keeps
    // its lookup index moving to each new shape; every index must match the one derived
    // from that shape's whole chain.
    let source = r#"
      var wide = {};
      for (var i = 0; i < 300; i++) {
        wide["k" + i] = i;
        if (i % 7 === 0) wide["k" + i];
        if (i % 5 === 0) Object.defineProperty(wide, "k" + i, { enumerable: false });
        if (i % 11 === 0) Object.defineProperty(wide, "g" + i, { get() { return i; } });
      }
      var keys = Object.keys(wide).length;
    "#;
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize(source, "wide.js").unwrap();
    vm.execute(&program).unwrap();
    let mut compared = 0;
    for shape in 0..vm.shapes.len() {
        let Some(index) = vm.shapes[shape].lookup_index.get() else {
            continue;
        };
        let derived = super::object::derive_shape_lookup_index(&vm.shapes, shape as u32);
        assert!(index.entries == derived.entries, "shape {shape} entries");
        assert!(index.attributes == derived.attributes, "shape {shape} attributes");
        compared += 1;
    }
    assert!(compared > 0, "the wide object's shapes keep an index");
}

#[test]
fn a_deletion_keeps_a_shared_shape_until_vacated_slots_outnumber_properties() {
    let source = r#"
      var first = { a: 1, b: 2, c: 3 };
      var second = { a: 4, b: 5, c: 6 };
      delete first.b;
      delete second.b;
      var emptied = { a: 1, b: 2 };
      delete emptied.a;
      delete emptied.b;
      print([first.a + first.c + second.a + second.c, "b" in first, Object.keys(second)].join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "deletion-shapes.js").unwrap();
    vm.execute(&program).unwrap();
    let global = vm.realm.globals;
    let shape_of = |vm: &mut Vm<TestHost>, name: &str| {
        let atom = vm.intern_atom(name);
        let object = vm.own_property(global, atom).expect("global binding exists");
        vm.object_data(object).expect("object value").shape()
    };
    let (first, second, emptied) =
        (shape_of(&mut vm, "first"), shape_of(&mut vm, "second"), shape_of(&mut vm, "emptied"));
    // One vacated slot beside two properties: both objects share the deletion's shape.
    assert!(!vm.shape_is_dictionary(first));
    assert_eq!(first, second);
    // Two vacated slots and no property: the object's storage is all holes.
    assert_eq!(
        vm.shapes[emptied as usize].dictionary_trigger,
        Some(super::DictionaryTrigger::DeletionPattern)
    );
    assert_eq!(output.borrow().as_slice(), ["14 false a,c"]);
}

#[test]
fn deleting_the_last_added_property_returns_to_the_earlier_shape() {
    let source = r#"
      var base = { a: 1 };
      var stack = { a: 1 };
      for (var id = 1; id <= 64; id++) { stack[id] = id; stack["k" + id] = id; delete stack["k" + id]; delete stack[id]; }
      print([Object.keys(stack), stack.a, "k1" in stack].join(" "));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "deletion-rollback.js").unwrap();
    let shapes_before = vm.shapes.len();
    vm.execute(&program).unwrap();
    let global = vm.realm.globals;
    let shape_of = |vm: &mut Vm<TestHost>, name: &str| {
        let atom = vm.intern_atom(name);
        let object = vm.own_property(global, atom).expect("global binding exists");
        vm.object_data(object).expect("object value").shape()
    };
    // Each key is deleted while it is the last one added: the object is back on the shape
    // it started with, and no deletion forked a shape of its own.
    assert_eq!(shape_of(&mut vm, "stack"), shape_of(&mut vm, "base"));
    let deletions = vm.shapes[shapes_before..]
        .iter()
        .filter(|shape| matches!(shape.transition, super::ShapeTransition::Delete { .. }))
        .count();
    assert_eq!(deletions, 0);
    assert_eq!(output.borrow().as_slice(), ["a 1 false"]);
}

#[test]
fn dictionary_shape_starts_when_property_slots_exceed_cache_encoding() {
    let mut vm = Vm::new(TestHost::default());
    vm.shapes[0].storage_len = super::object::FIELD_CACHE_SLOT_CAPACITY as u32;
    let atom = vm.intern_atom("overflow");
    let key = super::property_key::PropertyKey::string(atom);
    let shape = vm.transition_property_shape(0, key);
    assert!(vm.shape_is_dictionary(shape));
    assert_eq!(
        vm.shapes[shape as usize].dictionary_trigger,
        Some(super::DictionaryTrigger::PropertyCount)
    );
    assert_eq!(
        vm.property_shape_slot(shape, key),
        Some(super::object::FIELD_CACHE_SLOT_CAPACITY)
    );
    assert_eq!(
        vm.shapes[shape as usize].storage_len(),
        super::object::FIELD_CACHE_SLOT_CAPACITY + 1
    );
    #[cfg(feature = "profile-aggregate")]
    assert_eq!(
        vm.profile.dictionary_transitions[super::DictionaryTrigger::PropertyCount.index()],
        1
    );
}

#[test]
fn object_method_home_survives_collection_with_precise_register_roots() {
    let source = r#"
      var base = { answer: 42 };
      var receiver = {
        __proto__: base,
        answer() { return super.answer; }
      };
      var i = 0;
      while (i < 5000) { var temporary = {}; i = i + 1; }
      print(receiver.answer());
    "#;
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = compile(source, "object-home-gc.js").unwrap();
        if let Err(error) = vm.execute(&program) {
            panic!("{mode}: {}", vm.format_error(&program, &error));
        }
        assert_eq!(output.borrow().as_slice(), ["42"], "{mode}");
    }
}

#[test]
fn unrepresentable_register_maps_keep_the_conservative_frame_roots() {
    let mut program = Engine::specialize("print(0);", "wide-roots.js").unwrap();
    program.functions[0].registers = 65;
    program.functions[0].register_roots = crate::bytecode::RegisterRoots::new(false);

    let mut vm = Vm::new(TestHost::default());
    vm.initialize(&program).unwrap();
    let live = vm
        .heap
        .alloc(crate::heap::Cell::Error("live register".into()));
    let original = vm
        .heap
        .alloc(crate::heap::Cell::Error("original argument".into()));
    let mut registers = vec![Value::UNDEFINED; 65];
    registers[64] = live;
    vm.frames.push(Box::new(super::Frame {
        context: super::activation::CallContext::Internal,
        reflected_parameters: 0,
        original_arguments: vec![original],
        program: super::program_store::ProgramId::MAIN,
        function: 0,
        pc: 0,
        pc_is_resume: true,
        js_jit_skip_entry_once: false,
        source_field: None,
        binding_site_pc: None,
        env: Value::NULL,
        this: Value::UNDEFINED,
        locals: vec![],
        dynamic_bindings: vec![],
        captured: false,
        registers,
        initialized_registers: 65,
        capture_cache: super::CaptureCache::default(),
        active_iterators: vec![],
        with_objects: Vec::new(),
        with_base: 0,
        js_jit_interpreter_started: None,
        fixed_this: false,
    }));

    vm.collect_now(&program);
    assert!(vm.heap.is_live(live));
    assert!(vm.heap.is_live(original));

    vm.frames.pop();
    vm.collect_now(&program);
    assert!(!vm.heap.is_live(live));
    assert!(!vm.heap.is_live(original));
}

#[test]
fn untaken_closure_branch_does_not_allocate_environments() {
    const CALLS: usize = 1_000;
    fn environments(
        body: &str,
        make: bool,
        compile: fn(&str, &str) -> Result<crate::ResidualProgram, Vec<crate::Diagnostic>>,
    ) -> usize {
        let source = format!(
            "function maybe(make) {{ var value = 1; {body} return value; }} \
             var i = 0; while (i < {CALLS}) {{ maybe({make}); i = i + 1; }}"
        );
        let program = compile(&source, "lazy-env.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        vm.initialize(&program).unwrap();
        vm.heap.retain_allocations_for_test();
        let baseline = vm.heap.environment_count_for_test();
        let root = vm.closure(&program, 0, Value::NULL).unwrap();
        let globals = vm.realm.globals;
        vm.call_value(&program, root, globals, &[]).unwrap();
        assert_eq!(vm.heap.stats().1, 0, "census must retain every allocation");
        vm.heap.environment_count_for_test() - baseline
    }

    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let no_closure = environments("", false, compile);
        let closure_branch = "if (make) return function() { return value; };";
        let untaken = environments(closure_branch, false, compile);
        let taken = environments(closure_branch, true, compile);
        assert_eq!(untaken, no_closure, "untaken closure adds environments");
        assert_eq!(taken - untaken, CALLS, "one environment per taken closure");
    }
}

#[test]
fn third_method_receiver_promotes_site_to_megamorphic() {
    let mut vm = Vm::new(TestHost::default());
    vm.method_caches.push([super::EMPTY_METHOD_CACHE; 2]);
    for shape in 1..=4 {
        vm.record_method_cache(
            0,
            MethodCache {
                shape,
                atom: 0,
                proto: crate::Value::NULL,
                callee: crate::Value::NULL,
                guard: super::EMPTY_METHOD_GUARD,
                target: Some(CallTarget::User(
                    super::program_store::ProgramId::MAIN,
                    shape,
                    crate::Value::NULL,
                )),
            },
        );
    }
    assert_eq!(vm.megamorphic_methods[0].len, 4);
}

#[test]
fn method_cache_gc_retains_live_and_rejects_reused_handles() {
    let mut vm = Vm::new(TestHost::default());
    let live = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: crate::Value::NULL,
        function: u32::MAX,
        slots: Vec::<Value>::new().into_boxed_slice().into(),
        scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
            program: None,
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: vec![].into(),
            with_objects: Box::default(),
        }),
    });
    let dead = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: crate::Value::NULL,
        function: u32::MAX,
        slots: Vec::<Value>::new().into_boxed_slice().into(),
        scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
            program: None,
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: vec![].into(),
            with_objects: Box::default(),
        }),
    });
    vm.method_caches.push([
        MethodCache {
            shape: 1,
            atom: 0,
            proto: crate::Value::NULL,
            callee: crate::Value::NULL,
            guard: super::EMPTY_METHOD_GUARD,
            target: Some(CallTarget::User(
                super::program_store::ProgramId::MAIN,
                1,
                live,
            )),
        },
        MethodCache {
            shape: 2,
            atom: 0,
            proto: crate::Value::NULL,
            callee: crate::Value::NULL,
            guard: super::EMPTY_METHOD_GUARD,
            target: Some(CallTarget::User(
                super::program_store::ProgramId::MAIN,
                2,
                dead,
            )),
        },
    ]);
    vm.heap.collect([live]);
    vm.retain_live_method_caches();
    assert!(matches!(
        vm.method_caches[0][0].target,
        Some(CallTarget::User(_, 1, env)) if env == live
    ));
    assert!(vm.method_caches[0][1].target.is_none());

    let reused = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: crate::Value::NULL,
        function: u32::MAX,
        slots: Vec::<Value>::new().into_boxed_slice().into(),
        scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
            program: None,
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: vec![].into(),
            with_objects: Box::default(),
        }),
    });
    assert_eq!(reused, dead);
    assert!(vm.method_caches[0][1].target.is_none());
}


#[test]
fn array_like_list_roots_release_after_callback_completion() {
    use super::operations::ArrayLikeElementKind;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for property_keys in [false, true] {
            for phase in [
                "accept",
                "length-throw",
                "coercion-throw",
                "element-throw",
                "invalid",
                "reserve",
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    var marker = {{kind:'{phase}'}}, reads = 0;
                    function makeList() {{return {{get length() {{
                        $262.gc(); if ('{phase}' === 'length-throw') throw marker;
                        return {{valueOf() {{
                            $262.gc(); if ('{phase}' === 'coercion-throw') throw marker;
                            return '{phase}' === 'reserve' ? Infinity : 2;
                        }}}};
                    }}, get 0() {{
                        $262.gc(); reads++;
                        if ('{phase}' === 'invalid') return undefined;
                        return {property_keys} ? Symbol('first') : {{rank:42}};
                    }}, get 1() {{
                        $262.gc(); reads++;
                        if ('{phase}' === 'element-throw') throw marker;
                        return {property_keys} ? 'later' : {{rank:43}};
                    }}}};}}
                "#
                );
                let program = compile(&source, "array-like-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let make = vm.intern_atom("makeList");
                let make = vm.own_property(vm.realm.globals, make).unwrap();
                let list = vm
                    .call_value(&program, make, Value::UNDEFINED, &[])
                    .unwrap();
                let weak = vm.heap.weak_handle(list).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let kind = if property_keys {
                    ArrayLikeElementKind::PropertyKey
                } else {
                    ArrayLikeElementKind::Any
                };
                let result = vm.create_list_from_array_like(&program, list, kind);
                if phase == "accept" || (phase == "invalid" && !property_keys) {
                    let values = result.unwrap();
                    assert_eq!(values.len(), 2);
                    if phase == "invalid" {
                        assert!(values[0].is_undefined());
                    } else if property_keys {
                        assert!(matches!(
                            vm.heap.get(values[0]),
                            Some(super::Cell::Symbol(_))
                        ));
                    } else {
                        let rank = vm.intern_atom("rank");
                        assert_eq!(vm.own_property(values[0], rank), Some(Value::number(42.0)));
                    }
                    if property_keys {
                        assert_eq!(vm.to_string(&program, values[1]).unwrap(), "later");
                    } else {
                        let rank = vm.intern_atom("rank");
                        assert_eq!(vm.own_property(values[1], rank), Some(Value::number(43.0)));
                    }
                } else if phase == "reserve" || phase == "invalid" {
                    assert!(
                        vm.format_error(&program, &result.unwrap_err())
                            .contains("TypeError")
                    );
                } else {
                    let marker = vm.intern_atom("marker");
                    assert_eq!(
                        result.unwrap_err().thrown_value(),
                        vm.own_property(vm.realm.globals, marker)
                    );
                }
                let reads = vm.intern_atom("reads");
                let expected = if phase == "invalid" && property_keys {
                    1.0
                } else if matches!(phase, "accept" | "invalid" | "element-throw") {
                    2.0
                } else {
                    0.0
                };
                assert_eq!(
                    vm.own_property(vm.realm.globals, reads),
                    Some(Value::number(expected))
                );
                assert_eq!(
                    vm.heap.root_count_for_test(),
                    roots,
                    "{phase}/{property_keys}"
                );
                assert_eq!(vm.active_call_roots.len(), calls, "{phase}/{property_keys}");
                vm.collect_now(&program);
                assert!(
                    vm.heap.weak_value(weak).is_none(),
                    "{phase}/{property_keys}"
                );
            }
        }
    }
}

#[test]
fn proxy_own_key_roots_release_after_invariant_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "accept",
            "lookup-throw",
            "trap-throw",
            "result-throw",
            "extensible-throw",
            "target-keys-throw",
            "descriptor-throw",
            "omit",
            "duplicate",
            "invalid-type",
            "extra",
            "revoked",
            "fallback",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var marker = {{kind:'{phase}'}};
                function makeProxy() {{
                    var target = {{first:42, later:43}};
                    Object.defineProperty(target,'first',{{configurable:false}});
                    Object.preventExtensions(target);
                    target = new Proxy(target, {{isExtensible(object) {{
                        $262.gc(); if ('{phase}' === 'extensible-throw') throw marker;
                        return Reflect.isExtensible(object);
                    }}, ownKeys(object) {{
                        $262.gc(); if ('{phase}' === 'target-keys-throw') throw marker;
                        return Reflect.ownKeys(object);
                    }}, getOwnPropertyDescriptor(object,key) {{
                        $262.gc(); if ('{phase}' === 'descriptor-throw' && key === 'later') throw marker;
                        return Reflect.getOwnPropertyDescriptor(object,key);
                    }}}});
                    var handler = {{get ownKeys() {{
                        $262.gc(); if ('{phase}' === 'lookup-throw') throw marker;
                        if ('{phase}' === 'fallback') return null;
                        return function() {{
                            $262.gc(); if ('{phase}' === 'trap-throw') throw marker;
                            if ('{phase}' === 'omit') return ['later'];
                            if ('{phase}' === 'duplicate') return ['first','first'];
                            if ('{phase}' === 'extra') return ['first','later','extra'];
                            if ('{phase}' === 'invalid-type') return [undefined];
                            return {{length:2, get 0() {{return 'first';}}, get 1() {{
                                $262.gc(); if ('{phase}' === 'result-throw') throw marker; return 'later';
                            }}}};
                        }};
                    }}}};
                    if ('{phase}' === 'revoked') {{var revoked=Proxy.revocable(target,handler);revoked.revoke();return revoked.proxy;}}
                    return new Proxy(target,handler);
                }}
            "#
            );
            let program = compile(&source, "proxy-own-key-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let make = vm.intern_atom("makeProxy");
            let make = vm.own_property(vm.realm.globals, make).unwrap();
            let proxy = vm
                .call_value(&program, make, Value::UNDEFINED, &[])
                .unwrap();
            let weak = vm.heap.weak_handle(proxy).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.proxy_own_keys(&program, proxy);
            match phase {
                "accept" | "fallback" => {
                    let keys = result.unwrap();
                    assert_eq!(
                        keys.iter()
                            .map(|key| vm.to_string(&program, *key).unwrap())
                            .collect::<Vec<_>>(),
                        ["first", "later"]
                    );
                }
                "omit" | "duplicate" | "extra" | "invalid-type" | "revoked" => assert!(
                    vm.format_error(&program, &result.unwrap_err())
                        .contains("TypeError")
                ),
                _ => {
                    let marker = vm.intern_atom("marker");
                    assert_eq!(
                        result.unwrap_err().thrown_value(),
                        vm.own_property(vm.realm.globals, marker)
                    );
                }
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(weak).is_none(), "{phase}");
        }
    }
}

#[test]
fn own_enumeration_roots_release_after_callback_completion() {
    use super::object_keys::EnumerableOwnPropertyKind;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in ["keys", "values", "entries"] {
            for phase in ["accept", "own-keys-throw", "descriptor-throw", "get-throw"] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    var marker = {{kind:'{phase}'}};
                    function makeSource() {{return new Proxy({{}}, {{get ownKeys() {{
                        $262.gc(); return function() {{
                            $262.gc(); if ('{phase}' === 'own-keys-throw') throw marker;
                            return ['first','later',Symbol('ignored')];
                        }};
                    }}, getOwnPropertyDescriptor(object,key) {{
                        $262.gc(); if (key === 'later' && '{phase}' === 'descriptor-throw') throw marker;
                        return {{enumerable:true, configurable:true, writable:true, value:undefined}};
                    }}, get(object,key) {{
                        $262.gc(); if (key === 'later' && '{phase}' === 'get-throw') throw marker;
                        return {{rank:key === 'first' ? 42 : 43}};
                    }}}});}}
                "#
                );
                let program = compile(&source, "enumeration-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let make = vm.intern_atom("makeSource");
                let make = vm.own_property(vm.realm.globals, make).unwrap();
                let source = vm
                    .call_value(&program, make, Value::UNDEFINED, &[])
                    .unwrap();
                let source_weak = vm.heap.weak_handle(source).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let projection = match kind {
                    "keys" => EnumerableOwnPropertyKind::Key,
                    "values" => EnumerableOwnPropertyKind::Value,
                    _ => EnumerableOwnPropertyKind::KeyValue,
                };
                let result = vm.enumerable_own_properties(&program, source, projection);
                let succeeds = phase == "accept" || (phase == "get-throw" && kind == "keys");
                let result_weak = if succeeds {
                    let result = result.unwrap();
                    let values = vm.array_values(result).unwrap();
                    assert_eq!(values.len(), 2, "{kind}/{phase}");
                    for (index, value) in values.into_iter().enumerate() {
                        if kind == "keys" {
                            assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                ["first", "later"][index]
                            );
                        } else {
                            let value = if kind == "entries" {
                                let pair = vm.array_values(value).unwrap();
                                assert_eq!(
                                    vm.to_string(&program, pair[0]).unwrap(),
                                    ["first", "later"][index]
                                );
                                pair[1]
                            } else {
                                value
                            };
                            let rank = vm.intern_atom("rank");
                            assert_eq!(
                                vm.own_property(value, rank),
                                Some(Value::number(42.0 + index as f64))
                            );
                        }
                    }
                    Some(vm.heap.weak_handle(result).unwrap())
                } else {
                    let marker = vm.intern_atom("marker");
                    assert_eq!(
                        result.unwrap_err().thrown_value(),
                        vm.own_property(vm.realm.globals, marker)
                    );
                    None
                };
                assert_eq!(vm.heap.root_count_for_test(), roots, "{kind}/{phase}");
                assert_eq!(vm.active_call_roots.len(), calls, "{kind}/{phase}");
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(source_weak).is_none(), "{kind}/{phase}");
                if let Some(result_weak) = result_weak {
                    assert!(vm.heap.weak_value(result_weak).is_none(), "{kind}/{phase}");
                }
            }
        }
    }
}

#[test]
fn proxy_set_roots_release_after_collecting_completion() {
    use super::property_key::PropertyKey;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for symbol in [false, true] {
            for phase in [
                "accept",
                "reject",
                "lookup-throw",
                "trap-throw",
                "descriptor-throw",
                "frozen",
                "forward",
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    var events = [], marker = {{kind:'{phase}'}};
                    function makeOperands() {{
                        var key = {symbol} ? Symbol('entry') : 'entry';
                        var target = {{tag:41}};
                        if ('{phase}' === 'frozen') Object.defineProperty(target, key, {{value:99}});
                        target = new Proxy(target, {{getOwnPropertyDescriptor(object, key) {{
                            $262.gc(); events.push('descriptor');
                            if ('{phase}' === 'descriptor-throw') throw marker;
                            return Reflect.getOwnPropertyDescriptor(object, key);
                        }}}});
                        var handler = {{get set() {{
                            $262.gc(); events.push('lookup');
                            if ('{phase}' === 'lookup-throw') throw marker;
                            if ('{phase}' === 'forward') return undefined;
                            return function(target, key, value, receiver) {{
                                $262.gc(); events.push('trap');
                                if (target.tag !== 41 || value.rank !== 42 || receiver.rank !== 43
                                    || typeof key !== ({symbol} ? 'symbol' : 'string')) throw 'lost operand';
                                if ('{phase}' === 'trap-throw') throw marker;
                                return '{phase}' !== 'reject';
                            }};
                        }}}};
                        return [target, handler, {{rank:43}}, key, {{rank:42}}];
                    }}
                "#
                );
                let program = compile(&source, "proxy-set-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let make = vm.intern_atom("makeOperands");
                let make = vm.own_property(vm.realm.globals, make).unwrap();
                let operands = vm
                    .call_value(&program, make, Value::UNDEFINED, &[])
                    .unwrap();
                let [target, handler, receiver, key, value]: [Value; 5] =
                    vm.array_values(operands).unwrap().try_into().unwrap();
                let weak = vm.heap.weak_handle(value).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let property = if symbol {
                    PropertyKey::symbol(key)
                } else {
                    PropertyKey::string(vm.intern_atom("entry"))
                };
                let result = vm.proxy_set(&program, target, handler, receiver, property, value);
                match phase {
                    "accept" | "forward" => assert!(result.unwrap()),
                    "reject" => assert!(!result.unwrap()),
                    "frozen" => assert!(
                        vm.format_error(&program, &result.unwrap_err())
                            .contains("TypeError")
                    ),
                    _ => {
                        let marker = vm.intern_atom("marker");
                        assert_eq!(
                            result.unwrap_err().thrown_value(),
                            vm.own_property(vm.realm.globals, marker)
                        );
                    }
                }
                assert_eq!(
                    vm.heap.root_count_for_test(),
                    roots,
                    "{phase}, symbol={symbol}"
                );
                assert_eq!(
                    vm.active_call_roots.len(),
                    calls,
                    "{phase}, symbol={symbol}"
                );
                let rank = vm.intern_atom("rank");
                assert_eq!(
                    vm.own_property(value, rank),
                    Some(Value::number(42.0)),
                    "{phase}"
                );
                let events = vm.intern_atom("events");
                let events = vm.own_property(vm.realm.globals, events).unwrap();
                let observed = vm
                    .array_values(events)
                    .unwrap()
                    .iter()
                    .map(|value| vm.to_string(&program, *value).unwrap())
                    .collect::<Vec<_>>();
                let expected: &[&str] = match phase {
                    "lookup-throw" | "forward" => &["lookup"],
                    "reject" | "trap-throw" => &["lookup", "trap"],
                    _ => &["lookup", "trap", "descriptor"],
                };
                assert_eq!(observed, expected, "{phase}, symbol={symbol}");
                vm.collect_now(&program);
                assert!(
                    vm.heap.weak_value(weak).is_none(),
                    "{phase}, symbol={symbol}"
                );
            }
        }
    }
}

#[test]
fn receiver_proxy_writes_root_operands_across_traps() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for symbol in [false, true] {
            for existing in [false, true] {
                for phase in ["accept", "reject", "descriptor-throw", "define-throw"] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                    var marker = {{kind:'{phase}'}};
                    function makeOperands() {{
                        var key = {symbol} ? Symbol('entry') : 'entry', receiver = {{tag:41}};
                        if ({existing}) receiver[key] = 99;
                        receiver = new Proxy(receiver, {{getOwnPropertyDescriptor(object, key) {{
                            $262.gc();
                            if (({symbol} ? key.description : key) !== 'entry' || object.tag !== 41) throw 'lost operand';
                            if ('{phase}' === 'descriptor-throw') throw marker;
                            return Reflect.getOwnPropertyDescriptor(object, key);
                        }}, defineProperty(object, key, descriptor) {{
                            $262.gc();
                            if (({symbol} ? key.description : key) !== 'entry' || descriptor.value.rank !== 42
                                || Object.hasOwn(descriptor, 'writable') !== !{existing}) throw 'lost operand';
                            if ('{phase}' === 'define-throw') throw marker;
                            if ('{phase}' === 'reject') return false;
                            return Reflect.defineProperty(object, key, descriptor);
                        }}}});
                        return [{{}}, key, {{rank:42}}, receiver];
                    }}
                "#
                    );
                    let program = compile(&source, "symbol-receiver-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let make = vm.intern_atom("makeOperands");
                    let make = vm.own_property(vm.realm.globals, make).unwrap();
                    let operands = vm
                        .call_value(&program, make, Value::UNDEFINED, &[])
                        .unwrap();
                    let [target, key, value, receiver]: [Value; 4] =
                        vm.array_values(operands).unwrap().try_into().unwrap();
                    let weak = vm.heap.weak_handle(value).unwrap();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = if symbol {
                        vm.set_symbol_property_with_receiver(&program, target, key, value, receiver)
                    } else {
                        let atom = vm.intern_atom("entry");
                        vm.set_property_with_receiver(&program, target, atom, value, receiver)
                    };
                    match phase {
                        "accept" => assert!(result.unwrap()),
                        "reject" => assert!(!result.unwrap()),
                        _ => {
                            let marker = vm.intern_atom("marker");
                            assert_eq!(
                                result.unwrap_err().thrown_value(),
                                vm.own_property(vm.realm.globals, marker)
                            );
                        }
                    }
                    assert_eq!(
                        vm.heap.root_count_for_test(),
                        roots,
                        "{phase}, existing={existing}"
                    );
                    assert_eq!(
                        vm.active_call_roots.len(),
                        calls,
                        "{phase}, existing={existing}"
                    );
                    let rank = vm.intern_atom("rank");
                    assert_eq!(
                        vm.own_property(value, rank),
                        Some(Value::number(42.0)),
                        "{phase}"
                    );
                    vm.collect_now(&program);
                    assert!(
                        vm.heap.weak_value(weak).is_none(),
                        "{phase}, existing={existing}"
                    );
                }
            }
        }
    }
}

#[test]
fn legacy_accessor_record_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::ObjectPrototypeDefineGetter,
            Native::ObjectPrototypeDefineSetter,
        ] {
            for phase in [
                "success",
                "boxed",
                "key-throw",
                "invalid",
                "proxy-reject",
                "proxy-throw",
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    var marker = {{kind:'{phase}'}};
                    var target = '{phase}' === 'boxed' ? 1 : {{}};
                    if ('{phase}'.startsWith('proxy-')) target = new Proxy(target, {{defineProperty() {{
                        $262.gc(); if ('{phase}' === 'proxy-throw') throw marker; return false;
                    }}}});
                    var key = {{toString() {{$262.gc(); if ('{phase}' === 'key-throw') throw marker; return 'entry';}}}};
                    var makeAccessor = function() {{return function() {{return 42;}};}};
                "#
                );
                let program = compile(&source, "legacy-accessor-record-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let values = ["target", "key", "marker", "makeAccessor"].map(|name| {
                    let atom = vm.intern_atom(name);
                    vm.own_property(vm.realm.globals, atom).unwrap()
                });
                let accessor = vm
                    .call_value(&program, values[3], Value::UNDEFINED, &[])
                    .unwrap();
                let weak = vm.heap.weak_handle(accessor).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let argument = if phase == "invalid" {
                    Value::UNDEFINED
                } else {
                    accessor
                };
                let result = vm.object_prototype_define_accessor(
                    &program,
                    native,
                    values[0],
                    &[values[1], argument],
                );
                match phase {
                    "success" | "boxed" => assert_eq!(result.unwrap(), Value::UNDEFINED),
                    "invalid" | "proxy-reject" => {
                        let error = result.unwrap_err();
                        assert!(vm.format_error(&program, &error).contains("TypeError"));
                    }
                    _ => assert_eq!(result.unwrap_err().thrown_value(), Some(values[2])),
                }
                assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
                assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
                vm.collect_now(&program);
                assert_eq!(
                    vm.heap.weak_value(weak).is_some(),
                    phase == "success",
                    "{phase}"
                );
            }
        }
    }
}

#[test]
fn class_field_record_roots_release_after_each_completion() {
    use super::property_key::PropertyKey;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "ordinary",
            "symbol",
            "private",
            "reject",
            "proxy-reject",
            "proxy-throw",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var target = {{}}, marker = {{kind:'{phase}'}}, symbol = Symbol('entry');
                if ('{phase}' === 'reject') Object.preventExtensions(target);
                if ('{phase}'.startsWith('proxy-')) target = new Proxy(target, {{defineProperty() {{
                    $262.gc(); if ('{phase}' === 'proxy-throw') throw marker; return false;
                }}}});
                var makeValue = function() {{return {{rank:42}};}};
            "#
            );
            let program = compile(&source, "class-field-record-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let values = ["target", "marker", "symbol", "makeValue"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let atom = vm.intern_atom("entry");
            let key = match phase {
                "symbol" => PropertyKey::symbol(values[2]),
                "private" => PropertyKey::private(atom),
                _ => PropertyKey::string(atom),
            };
            let value = vm
                .call_value(&program, values[3], Value::UNDEFINED, &[])
                .unwrap();
            let weak = vm.heap.weak_handle(value).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.define_class_field(&program, values[0], key, value);
            match phase {
                "ordinary" | "symbol" => result.unwrap(),
                "proxy-throw" => assert_eq!(result.unwrap_err().thrown_value(), Some(values[1])),
                "private" => assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("private names are not public class-field keys")
                ),
                _ => {
                    let error = result.unwrap_err();
                    assert!(vm.format_error(&program, &error).contains("TypeError"));
                }
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
            vm.collect_now(&program);
            assert_eq!(
                vm.heap.weak_value(weak).is_some(),
                matches!(phase, "ordinary" | "symbol"),
                "{phase}"
            );
        }
    }
}

#[test]
fn json_revival_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "reviver-throw",
            "get-throw",
            "keys-throw",
            "descriptor-throw",
            "is-array-throw",
            "length-throw",
            "define-throw",
            "delete-throw",
            "success",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var marker = {{kind:'{phase}'}};
                var revive = function(key,value) {{
                    $262.gc();
                    if (key === 'first') {{
                        if ('{phase}' === 'get-throw') this.target = {{get entry() {{$262.gc(); throw marker;}}}};
                        if ('{phase}' === 'keys-throw') this.target = new Proxy({{}}, {{ownKeys() {{$262.gc(); throw marker;}}}});
                        if ('{phase}' === 'descriptor-throw') this.target = new Proxy({{entry:1}}, {{getOwnPropertyDescriptor() {{$262.gc(); throw marker;}}}});
                        if ('{phase}' === 'is-array-throw') {{var revoked = Proxy.revocable({{}},{{}}); revoked.revoke(); this.target = revoked.proxy;}}
                        if ('{phase}' === 'length-throw') this.target = new Proxy([], {{get(object,key) {{$262.gc(); if (key === 'length') throw marker; return Reflect.get(object,key);}}}});
                        if ('{phase}' === 'define-throw') this.target = new Proxy({{entry:1}}, {{defineProperty() {{$262.gc(); throw marker;}}}});
                        if ('{phase}' === 'delete-throw') this.target = new Proxy({{entry:1}}, {{deleteProperty() {{$262.gc(); throw marker;}}}});
                    }}
                    if (key === 'entry') {{
                        if ('{phase}' === 'reviver-throw') throw marker;
                        if ('{phase}' === 'delete-throw') return undefined;
                        return {{rank:42}};
                    }}
                    return value;
                }};
            "#
            );
            let program = compile(&source, "json-revival-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let values = ["revive", "marker"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let text = vm.heap.alloc_string(
                r#"{"first":0,"target":{"entry":1}}"#.into(),
            );
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.json_parse(&program, &[text, values[0]]);
            let weak = match phase {
                "success" => {
                    let result = result.unwrap();
                    let target = vm.intern_atom("target");
                    let target = vm.own_property(result, target).unwrap();
                    let entry = vm.intern_atom("entry");
                    let entry = vm.own_property(target, entry).unwrap();
                    let rank = vm.intern_atom("rank");
                    assert_eq!(vm.own_property(entry, rank), Some(Value::number(42.0)));
                    Some(vm.heap.weak_handle(result).unwrap())
                }
                "is-array-throw" => {
                    let error = result.unwrap_err();
                    assert!(vm.format_error(&program, &error).contains("TypeError"));
                    None
                }
                _ => {
                    assert_eq!(
                        result.unwrap_err().thrown_value(),
                        Some(values[1]),
                        "{phase}"
                    );
                    None
                }
            };
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
            vm.collect_now(&program);
            if let Some(weak) = weak {
                assert!(vm.heap.weak_value(weak).is_none());
            }
        }
    }
}

#[test]
fn definition_batch_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "own-keys-throw",
            "own-descriptor-throw",
            "get-descriptor-throw",
            "parse-throw",
            "invalid-late",
            "define-reject",
            "define-throw",
            "success",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var weak, marker = {{kind:'{phase}'}};
                var backing = {{}};
                var target = new Proxy(backing, {{defineProperty(object,key,descriptor) {{
                    $262.gc();
                    if (key === 'second' && '{phase}' === 'define-reject') return false;
                    if (key === 'second' && '{phase}' === 'define-throw') throw marker;
                    return Reflect.defineProperty(object,key,descriptor);
                }}}});
                var descriptors = new Proxy({{
                    first: {{get value() {{var value = {{rank:42}}; weak = new WeakRef(value); $262.gc(); return value;}}}},
                    second: {{get value() {{$262.gc(); if ('{phase}' === 'parse-throw') throw marker; return 43;}}}}
                }}, {{
                    ownKeys() {{$262.gc(); if ('{phase}' === 'own-keys-throw') throw marker; return ['first','second'];}},
                    getOwnPropertyDescriptor(object,key) {{$262.gc();
                        if (key === 'second' && '{phase}' === 'own-descriptor-throw') throw marker;
                        return {{enumerable:true, configurable:true}};
                    }},
                    get(object,key) {{$262.gc();
                        if (key === 'second' && '{phase}' === 'get-descriptor-throw') throw marker;
                        if (key === 'second' && '{phase}' === 'invalid-late') return {{get:1}};
                        return Reflect.get(object,key);
                    }}
                }});
            "#
            );
            let program = compile(&source, "definition-batch-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let values = ["target", "descriptors", "marker"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.object_define_properties(&program, &values[..2]);
            // End this direct host job before checking temporary-root cleanup.
            vm.clear_kept_objects();
            match phase {
                "success" => assert_eq!(result.unwrap(), values[0]),
                "invalid-late" | "define-reject" => {
                    let error = result.unwrap_err();
                    assert!(vm.format_error(&program, &error).contains("TypeError"));
                }
                _ => assert_eq!(result.unwrap_err().thrown_value(), Some(values[2])),
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls, "{phase}");
            let weak = vm.intern_atom("weak");
            let weak = vm.own_property(vm.realm.globals, weak).unwrap();
            let weak = match vm.heap.get(weak) {
                Some(super::Cell::WeakRef { target, .. }) => *target,
                _ => None,
            };
            vm.collect_now(&program);
            if phase != "own-keys-throw" {
                let weak = weak.expect("first descriptor created a weak value tracker");
                assert_eq!(
                    vm.heap.weak_value(weak).is_some(),
                    matches!(phase, "success" | "define-reject" | "define-throw"),
                    "{phase}"
                );
            }
        }
    }
}

#[test]
fn definition_record_stack_exhaustion_preserves_root_scopes() {
    let mut vm = Vm::new(TestHost::default().with_test262());
    let program = Engine::specialize("var target = {};", "definition-stack-budget.js").unwrap();
    vm.execute(&program).unwrap();
    let atom = vm.intern_atom("target");
    let target = vm.own_property(vm.realm.globals, atom).unwrap();
    let key = vm.heap.alloc_string("entry".into());
    let record = super::object_descriptors::PropertyDescriptorRecord::data(Value::number(42.0));
    let roots = vm.heap.root_count_for_test();
    let calls = vm.active_call_roots.len();
    let mut guards = Vec::new();
    while let Ok(guard) = quench_stack::StackGuard::enter() {
        guards.push(guard);
    }
    let error = vm
        .define_own_property_record(&program, target, key, record)
        .unwrap_err();
    drop(guards);
    assert!(
        vm.format_error(&program, &error)
            .contains(quench_stack::STACK_EXHAUSTED_MESSAGE)
    );
    assert_eq!(vm.heap.root_count_for_test(), roots);
    assert_eq!(vm.active_call_roots.len(), calls);
    assert!(
        vm.define_own_property_record(&program, target, key, record)
            .unwrap()
    );
}

#[test]
fn definition_record_roots_release_across_target_kinds() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "ordinary-ok",
            "ordinary-reject",
            "array-ok",
            "array-reject",
            "length-ok",
            "length-reject",
            "length-throw",
            "typed-ok",
            "typed-reject",
            "typed-throw",
            "proxy-ok",
            "proxy-reject",
            "proxy-throw",
            "proxy-invariant",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var target={{}};
                if ('{phase}'.startsWith('array-') || '{phase}'.startsWith('length-')) target=[];
                if ('{phase}'.startsWith('typed-')) target=new Uint8Array('{phase}' === 'typed-reject' ? 0 : 1);
                if ('{phase}' === 'ordinary-reject' || '{phase}' === 'array-reject' || '{phase}' === 'length-reject') Object.freeze(target);
                if ('{phase}'.startsWith('proxy-')) target=new Proxy(target, {{get defineProperty() {{$262.gc();
                    return function(target,key,descriptor) {{$262.gc(); if ('{phase}' === 'proxy-throw') throw {{kind:'{phase}'}}; return '{phase}' !== 'proxy-reject';}};
                }}}});
                var key='{phase}'.startsWith('length-') ? 'length' : '{phase}'.startsWith('array-') || '{phase}'.startsWith('typed-') ? '0' : 'entry';
                var descriptor={{get value() {{$262.gc(); return {{rank:42, valueOf() {{$262.gc();
                    if ('{phase}' === 'length-throw' || '{phase}' === 'typed-throw') throw {{kind:'{phase}'}}; return 2;
                }}}};}}, writable:true, enumerable:!'{phase}'.startsWith('length-'), configurable:!'{phase}'.startsWith('length-') && '{phase}' !== 'proxy-invariant'}};
            "#
            );
            let program = compile(&source, "definition-record-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let values = ["target", "key", "descriptor"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let record = vm.to_property_descriptor(&program, values[2]).unwrap();
            let weak = vm.heap.weak_handle(record.value.unwrap()).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.define_own_property_record(&program, values[0], values[1], record);
            if phase.ends_with("-throw") || phase == "proxy-invariant" {
                let error = result.unwrap_err();
                if phase == "proxy-invariant" {
                    assert!(vm.format_error(&program, &error).contains("TypeError"));
                } else {
                    let atom = vm.intern_atom("kind");
                    let kind = vm
                        .own_property(error.thrown_value().unwrap(), atom)
                        .unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                }
            } else {
                assert_eq!(result.unwrap(), phase.ends_with("-ok"), "{phase}");
            }
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            vm.collect_now(&program);
            assert_eq!(
                vm.heap.weak_value(weak).is_some(),
                matches!(phase, "ordinary-ok" | "array-ok"),
                "{phase}"
            );
        }
    }
}

#[test]
fn descriptor_record_roots_release_after_field_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "data-success",
            "accessor-success",
            "writable-throw",
            "get-throw",
            "set-throw",
            "invalid-get",
            "invalid-set",
            "mixed",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var descriptor = '{phase}' === 'data-success' || '{phase}' === 'writable-throw' || '{phase}' === 'mixed'
                    ? {{get value() {{$262.gc(); return {{rank:42}};}},
                        get writable() {{$262.gc(); if ('{phase}' === 'writable-throw') throw {{kind:'{phase}'}}; return true;}}}}
                    : {{}};
                if ('{phase}' !== 'data-success' && '{phase}' !== 'writable-throw') {{
                    Object.defineProperty(descriptor, 'get', {{get() {{$262.gc();
                        if ('{phase}' === 'get-throw') throw {{kind:'{phase}'}};
                        if ('{phase}' === 'invalid-get') return 1;
                        return function() {{return 42;}};
                    }}}});
                    Object.defineProperty(descriptor, 'set', {{get() {{$262.gc();
                        if ('{phase}' === 'set-throw') throw {{kind:'{phase}'}};
                        if ('{phase}' === 'invalid-set') return 1;
                        return function(value) {{this.saved = value;}};
                    }}}});
                }}
            "#
            );
            let program = compile(&source, "descriptor-record-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("descriptor");
            let descriptor = vm.own_property(vm.realm.globals, atom).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.to_property_descriptor(&program, descriptor);
            assert_eq!(result.is_ok(), phase.ends_with("success"), "{phase}");
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            if let Err(error) = &result {
                if phase.ends_with("-throw") {
                    let thrown = error.thrown_value().unwrap();
                    let atom = vm.intern_atom("kind");
                    let kind = vm.own_property(thrown, atom).unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                } else {
                    assert!(vm.format_error(&program, error).contains("TypeError"));
                }
            }
            if let Ok(record) = result {
                let value = if phase == "data-success" {
                    let value = record.value.unwrap();
                    let atom = vm.intern_atom("rank");
                    assert_eq!(vm.own_property(value, atom), Some(Value::number(42.0)));
                    value
                } else {
                    let getter = record.getter.unwrap();
                    assert_eq!(
                        vm.call_value(&program, getter, Value::UNDEFINED, &[])
                            .unwrap(),
                        Value::number(42.0)
                    );
                    getter
                };
                let weak = vm.heap.weak_handle(value).unwrap();
                let projection = vm.from_property_descriptor(record).unwrap();
                assert_eq!(vm.heap.root_count_for_test(), roots);
                let projection = vm.heap.weak_handle(projection).unwrap();
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(weak).is_none());
                assert!(vm.heap.weak_value(projection).is_none());
            }
        }
    }
}

#[test]
fn error_stack_setter_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "missing",
            "data",
            "accessor",
            "empty-accessor",
            "getter-only",
            "readonly",
            "nonextensible",
            "home",
            "primitive",
            "nonstring",
            "descriptor-lookup-throw",
            "descriptor-call-throw",
            "define-lookup-throw",
            "define-call-throw",
            "define-false",
            "set-false",
            "set-call-throw",
            "accessor-throw",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var marker = {{rank:42}};
                function factory() {{
                    if ('{phase}' === 'home') return Error.prototype;
                    if ('{phase}' === 'primitive') return 1;
                    var target = {{}};
                    if ('{phase}' === 'data' || '{phase}' === 'readonly' || '{phase}'.startsWith('set-'))
                        Object.defineProperty(target, 'stack', {{value:'old', writable:'{phase}' !== 'readonly', configurable:true}});
                    if ('{phase}' === 'accessor' || '{phase}' === 'accessor-throw')
                        Object.defineProperty(target, 'stack', {{set(v) {{$262.gc(); if ('{phase}' === 'accessor-throw') throw marker; this.saved = v;}}}});
                    if ('{phase}' === 'empty-accessor') Object.defineProperty(target, 'stack', {{get:undefined, set:undefined}});
                    if ('{phase}' === 'getter-only') Object.defineProperty(target, 'stack', {{get() {{return 'old';}}}});
                    if ('{phase}' === 'nonextensible') Object.preventExtensions(target);
                    if ('{phase}'.startsWith('descriptor-') || '{phase}'.startsWith('define-') || '{phase}'.startsWith('set-'))
                        return new Proxy(target, {{
                            get getOwnPropertyDescriptor() {{$262.gc(); if ('{phase}' === 'descriptor-lookup-throw') throw marker;
                                return function(t,k) {{$262.gc(); if ('{phase}' === 'descriptor-call-throw') throw marker; return Reflect.getOwnPropertyDescriptor(t,k);}};
                            }},
                            get defineProperty() {{$262.gc(); if ('{phase}' === 'define-lookup-throw') throw marker;
                                return function(t,k,d) {{$262.gc(); if ('{phase}' === 'define-call-throw') throw marker;
                                    if ('{phase}' === 'define-false') return false; return Reflect.defineProperty(t,k,d);}};
                            }},
                            set(t,k,v) {{$262.gc(); if ('{phase}' === 'set-call-throw') throw marker;
                                if ('{phase}' === 'set-false') return false; return Reflect.set(t,k,v,t);
                            }}
                        }});
                    return target;
                }}
            "#
            );
            let program = compile(&source, "error-stack-root-lifecycle.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("factory");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let target = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let hold = vm.heap.root(target);
            let value = if phase == "nonstring" {
                Value::number(1.0)
            } else {
                vm.heap
                    .alloc_string("fresh-stack".into())
            };
            let target = vm.heap.root_value(hold).unwrap();
            let target_weak = vm.heap.weak_handle(target);
            let value_weak = vm.heap.weak_handle(value);
            vm.heap.release_root(hold);
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let outcome = vm.error_stack_setter(&program, target, &[value]);
            // A borrowed V8 stack setter ignores receivers without captured
            // stack data, including proxies, primitives and arbitrary accessors.
            // It must not invoke their descriptor or setter hooks.
            assert!(outcome.is_ok(), "{phase}");
            assert_eq!(vm.heap.root_count_for_test(), roots, "{phase}");
            assert_eq!(vm.active_call_roots.len(), calls);
            vm.collect_now(&program);
            if let Some(weak) = target_weak {
                assert_eq!(
                    vm.heap.weak_value(weak).is_some(),
                    phase == "home",
                    "{phase}"
                );
            }
            if let Some(weak) = value_weak {
                assert!(vm.heap.weak_value(weak).is_none(), "{phase}");
            }
        }
    }
}

#[test]
fn module_namespace_operations_share_uninitialized_export_errors() {
    for compile in [
        Engine::specialize_module as fn(&str, &str) -> _,
        Engine::specialize_module_unspecialized,
    ] {
        for operation in [
            "get",
            "descriptor",
            "define",
            "seal",
            "freeze",
            "is-sealed",
            "is-frozen",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let program = compile("export let answer;", "uninitialized-module-export.mjs").unwrap();
            vm.initialize(&program).unwrap();
            let namespace = vm.object();
            // A namespace takes its own dictionary shape, as `Vm::module_namespace` gives it.
            vm.mark_object_dictionary(namespace, super::DictionaryTrigger::ModuleNamespace);
            let atom = vm.intern_atom("answer");
            vm.set_property(namespace, atom, Value::UNDEFINED).unwrap();
            let slot = program.function(0)
                .local_atoms
                .iter()
                .position(|candidate| *candidate == atom)
                .unwrap();
            let environment = vm.heap.alloc(crate::heap::Cell::Environment {
                parent: Value::NULL,
                function: super::ROOT_FUNCTION_ID,
                slots: vec![Value::DELETED; program.function(0).local_atoms.len()]
                    .into_boxed_slice()
                    .into(),
                scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                    program: Some(super::program_store::ProgramId::MAIN.raw()),
                    root_eval_scope: false,
                    binding_site_pc: None,
                    dynamic_bindings: vec![].into(),
                    with_objects: Box::default(),
                }),
            });
            vm.programs
                .set_module_environment(super::program_store::ProgramId::MAIN, environment);
            let attributes = super::PropertyAttributes {
                configurable: false,
                ..super::DEFAULT_PROPERTY_ATTRIBUTES
            };
            vm.set_property_attributes(
                namespace,
                super::property_key::PropertyKey::string(atom),
                attributes,
            );
            let object = vm.object_data_mut(namespace).unwrap();
            object.set_proto(Value::NULL);
            object.set_module_namespace();
            object.set_module_bindings(vec![(
                atom,
                super::program_store::ProgramId::MAIN,
                u16::try_from(slot).unwrap(),
            )]);
            object.set_extensible(false);
            let key = vm.heap.alloc_string("answer".into());
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let outcome = match operation {
                "get" => vm.get_property(&program, namespace, atom),
                "descriptor" => vm.object_get_own_property_descriptor(&program, &[namespace, key]),
                "define" => vm
                    .define_own_property_record(
                        &program,
                        namespace,
                        key,
                        super::object_descriptors::PropertyDescriptorRecord::data(Value::number(
                            42.0,
                        )),
                    )
                    .map(Vm::<TestHost>::integrity_bool),
                "seal" | "freeze" => {
                    vm.object_set_integrity(&program, &[namespace], operation == "freeze")
                }
                "is-sealed" | "is-frozen" => {
                    vm.object_is_integrity_level(&program, &[namespace], operation == "is-frozen")
                }
                _ => unreachable!(),
            };
            let error = outcome.unwrap_err();
            assert!(
                vm.format_error(&program, &error)
                    .starts_with("ReferenceError:"),
                "{operation}"
            );
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
        }
    }
}

#[test]
fn integrity_operation_roots_release_after_each_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for freeze in [false, true] {
            for test in [false, true] {
                let phases: &[&str] = if test {
                    &[
                        "success",
                        "extensible",
                        "extensible-lookup-throw",
                        "extensible-call-throw",
                        "keys-lookup-throw",
                        "keys-call-throw",
                        "descriptor-lookup-throw",
                        "descriptor-call-throw",
                        "configurable",
                        "writable",
                        "primitive",
                        "revoked",
                    ]
                } else {
                    &[
                        "success",
                        "prevent-lookup-throw",
                        "prevent-call-throw",
                        "prevent-false",
                        "keys-lookup-throw",
                        "keys-call-throw",
                        "descriptor-lookup-throw",
                        "descriptor-call-throw",
                        "define-lookup-throw",
                        "define-call-throw",
                        "define-false",
                        "primitive",
                        "revoked",
                    ]
                };
                for &phase in phases {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                        var marker = {{rank:42}};
                        function factory() {{
                            if ('{phase}' === 'primitive') return 1;
                            var target = {{alpha:42, beta:43}};
                            if ({test} && '{phase}' !== 'extensible') {{
                                Object.defineProperty(target, 'alpha', {{configurable:'{phase}' === 'configurable', writable:!{freeze} || '{phase}' === 'writable'}});
                                Object.defineProperty(target, 'beta', {{configurable:false, writable:!{freeze}}});
                                Object.preventExtensions(target);
                            }}
                            var handler = {{
                                get preventExtensions() {{$262.gc(); if ('{phase}' === 'prevent-lookup-throw') throw marker;
                                    return function(t) {{$262.gc(); if ('{phase}' === 'prevent-call-throw') throw marker;
                                        if ('{phase}' === 'prevent-false') return false;
                                        return Reflect.preventExtensions(t);
                                    }};
                                }},
                                get isExtensible() {{$262.gc(); if ('{phase}' === 'extensible-lookup-throw') throw marker;
                                    return function(t) {{$262.gc(); if ('{phase}' === 'extensible-call-throw') throw marker; return Reflect.isExtensible(t);}};
                                }},
                                get ownKeys() {{$262.gc(); if ('{phase}' === 'keys-lookup-throw') throw marker;
                                    return function(t) {{$262.gc(); if ('{phase}' === 'keys-call-throw') throw marker; return ['al' + 'pha', 'be' + 'ta'];}};
                                }},
                                get getOwnPropertyDescriptor() {{$262.gc(); if ('{phase}' === 'descriptor-lookup-throw') throw marker;
                                    return function(t,k) {{$262.gc(); if ('{phase}' === 'descriptor-call-throw') throw marker; return Reflect.getOwnPropertyDescriptor(t,k);}};
                                }},
                                get defineProperty() {{$262.gc(); if ('{phase}' === 'define-lookup-throw') throw marker;
                                    return function(t,k,d) {{$262.gc(); if ('{phase}' === 'define-call-throw') throw marker;
                                        if ('{phase}' === 'define-false') return false;
                                        return Reflect.defineProperty(t,k,d);
                                    }};
                                }}
                            }};
                            if ('{phase}' === 'revoked') {{var revocable = Proxy.revocable(target, handler); revocable.revoke(); return revocable.proxy;}}
                            return new Proxy(target, handler);
                        }}
                    "#
                    );
                    let program = compile(&source, "integrity-root-lifecycle.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("factory");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let object = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let weak = vm.heap.weak_handle(object);
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = if test {
                        vm.object_is_integrity_level(&program, &[object], freeze)
                    } else {
                        vm.object_set_integrity(&program, &[object], freeze)
                    };
                    let descriptor_skipped = !test && !freeze && phase.starts_with("descriptor-");
                    let fails = phase.ends_with("-throw") && !descriptor_skipped
                        || phase == "revoked"
                        || phase == "prevent-false"
                        || phase == "define-false";
                    assert_eq!(
                        outcome.is_err(),
                        fails,
                        "test={test} freeze={freeze} phase={phase}"
                    );
                    if let Err(error) = outcome {
                        if phase.ends_with("-throw") {
                            let atom = vm.intern_atom("marker");
                            assert_eq!(
                                error.thrown_value(),
                                vm.own_property(vm.realm.globals, atom)
                            );
                        }
                    } else if test {
                        let expected = phase != "extensible"
                            && phase != "configurable"
                            && !(freeze && phase == "writable");
                        assert_eq!(
                            outcome.unwrap(),
                            Vm::<TestHost>::integrity_bool(expected),
                            "{phase}"
                        );
                    } else {
                        assert_eq!(outcome.unwrap(), object);
                    }
                    assert_eq!(
                        vm.heap.root_count_for_test(),
                        roots,
                        "test={test} freeze={freeze} phase={phase}"
                    );
                    assert_eq!(vm.active_call_roots.len(), calls);
                    if let Some(weak) = weak {
                        vm.collect_now(&program);
                        assert!(
                            vm.heap.weak_value(weak).is_none(),
                            "test={test} freeze={freeze} phase={phase}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn reflect_definition_roots_release_after_callback_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "success",
            "proxy-success",
            "proxy-forward",
            "proxy-revoke-during-getter",
            "proxy-false",
            "proxy-getter-throw",
            "proxy-call-throw",
            "proxy-revoked",
            "descriptor-throw",
            "key-throw",
            "primitive-target",
            "invalid-get",
            "mixed",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var target = '{phase}' === 'primitive-target' ? 1 : {{}};
                if ('{phase}' === 'proxy-revoke-during-getter') {{
                    var revocable = Proxy.revocable(target, {{get defineProperty() {{
                        revocable.revoke(); $262.gc(); return function(target, key, descriptor) {{$262.gc(); return true;}};
                    }}}}); target = revocable.proxy;
                }} else if ('{phase}'.startsWith('proxy-')) {{
                    if ('{phase}' === 'proxy-revoked') {{var revocable = Proxy.revocable(target, {{}}); target = revocable.proxy; revocable.revoke();}}
                    else target = new Proxy(target, {{get defineProperty() {{$262.gc();
                        if ('{phase}' === 'proxy-forward') return undefined;
                        if ('{phase}' === 'proxy-getter-throw') throw {{kind:'{phase}'}};
                        return function(target, key, descriptor) {{$262.gc();
                            if ('{phase}' === 'proxy-call-throw') throw {{kind:'{phase}'}};
                            return '{phase}' !== 'proxy-false';
                        }};
                    }}}});
                }}
                var key = {{[Symbol.toPrimitive]() {{$262.gc(); if ('{phase}' === 'key-throw') throw {{kind:'{phase}'}}; return Symbol('entry');}}}};
                var descriptor = {{get value() {{$262.gc(); if ('{phase}' === 'descriptor-throw') throw {{kind:'{phase}'}}; return {{rank:42}};}},
                    get writable() {{$262.gc(); return true;}}, configurable:true}};
                if ('{phase}' === 'invalid-get' || '{phase}' === 'mixed')
                    Object.defineProperty(descriptor, 'get', {{get() {{$262.gc(); return '{phase}' === 'invalid-get' ? 1 : undefined;}}}});
            "#
            );
            let program = compile(&source, "reflect-definition-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let args = ["target", "key", "descriptor"].map(|name| {
                let atom = vm.intern_atom(name);
                vm.own_property(vm.realm.globals, atom).unwrap()
            });
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.reflect_define_property(&program, &args);
            let succeeds = matches!(
                phase,
                "success"
                    | "proxy-success"
                    | "proxy-false"
                    | "proxy-forward"
                    | "proxy-revoke-during-getter"
            );
            assert_eq!(result.is_ok(), succeeds, "{phase}");

            if let Err(error) = &result {
                if phase.ends_with("-throw") {
                    let thrown = error.thrown_value().unwrap();
                    let atom = vm.intern_atom("kind");
                    let kind = vm.own_property(thrown, atom).unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                } else {
                    assert!(vm.format_error(&program, error).contains("TypeError"));
                }
            }
            if succeeds {
                assert_eq!(
                    result.unwrap(),
                    Vm::<TestHost>::integrity_bool(phase != "proxy-false")
                );
            }
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
        }
    }
}

#[test]
fn from_entries_roots_release_after_iterator_and_entry_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "success",
            "next-getter",
            "next",
            "done",
            "value",
            "entry",
            "key",
            "entry-value",
            "convert",
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var source = {{[Symbol.iterator]() {{var index = 0; return {{
                    get next() {{$262.gc(); if ('{phase}' === 'next-getter') throw {{kind:'{phase}'}};
                        return function() {{$262.gc(); if ('{phase}' === 'next') throw {{kind:'{phase}'}};
                            if (index++) return {{done:true}};
                            return {{get done() {{$262.gc(); if ('{phase}' === 'done') throw {{kind:'{phase}'}}; return false;}},
                                get value() {{$262.gc(); if ('{phase}' === 'value') throw {{kind:'{phase}'}};
                                    if ('{phase}' === 'entry') return 1;
                                    return {{get 0() {{$262.gc(); if ('{phase}' === 'key') throw {{kind:'{phase}'}};
                                        return {{[Symbol.toPrimitive]() {{$262.gc(); if ('{phase}' === 'convert') throw {{kind:'{phase}'}}; return '__proto__';}}}};}},
                                        get 1() {{$262.gc(); if ('{phase}' === 'entry-value') throw {{kind:'{phase}'}}; return {{rank:42}};}}}};}}
                            }};
                        }};
                    }},
                    get return() {{$262.gc(); return function() {{$262.gc(); throw {{kind:'close'}};}};}}
                }};}}}};
            "#
            );
            let program = compile(&source, "from-entries-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("source");
            let source = vm.own_property(vm.realm.globals, atom).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.object_from_entries(&program, source);
            assert_eq!(result.is_err(), phase != "success", "{phase}");
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            match result {
                Ok(result) => {
                    let weak = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(weak).is_none());
                }
                Err(error) if phase != "entry" => {
                    let error = error.thrown_value().unwrap();
                    let atom = vm.intern_atom("kind");
                    let kind = vm.own_property(error, atom).unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                }
                Err(error) => assert!(vm.format_error(&program, &error).contains("TypeError")),
            }
        }
    }
}

#[test]
fn group_by_roots_release_after_iterator_and_callback_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in [super::GroupByKind::Object, super::GroupByKind::Map] {
            for phase in [
                "success",
                "next-getter",
                "next",
                "done",
                "value",
                "callback",
                "key",
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    var source = {{[Symbol.iterator]() {{var index = 0; return {{
                        get next() {{$262.gc(); if ('{phase}' === 'next-getter') throw {{kind:'next-getter'}}; return function() {{
                            $262.gc(); if ('{phase}' === 'next') throw {{kind:'next'}};
                            if (index++) return {{done:true}};
                            return {{get done() {{$262.gc(); if ('{phase}' === 'done') throw {{kind:'done'}}; return false;}},
                                get value() {{$262.gc(); if ('{phase}' === 'value') throw {{kind:'value'}}; return {{rank:42}};}}}};
                        }};}},
                        get return() {{$262.gc(); return function() {{$262.gc(); throw {{kind:'close'}};}};}}
                    }};}}}};
                    var callback = function(value) {{$262.gc(); if ('{phase}' === 'callback') throw {{kind:'callback'}};
                        return {{[Symbol.toPrimitive]() {{$262.gc(); if ('{phase}' === 'key') throw {{kind:'key'}}; return 'group';}}}};}};
                "#
                );
                let program = compile(&source, "group-by-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let callback_atom = vm.intern_atom("callback");
                let callback = vm.own_property(vm.realm.globals, callback_atom).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.group_by(&program, &[source, callback], kind);
                let fails = phase != "success"
                    && !(phase == "key" && matches!(kind, super::GroupByKind::Map));
                assert_eq!(result.is_err(), fails, "{phase}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                if let Ok(result) = result {
                    let weak = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(weak).is_none());
                } else if let Err(error) = result {
                    let error = error.thrown_value().unwrap();
                    let kind_atom = vm.intern_atom("kind");
                    let value = vm.own_property(error, kind_atom).unwrap();
                    assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                }
            }
        }
    }
}

#[test]
fn dynamic_constructor_roots_release_after_prototype_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::Function,
            Native::AsyncFunction,
            Native::GeneratorFunction,
            Native::AsyncGeneratorFunction,
        ] {
            for (action, fails, intrinsic) in [
                ("return prototype", false, false),
                ("return null", false, true),
                ("throw new Error('prototype')", true, false),
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let program = compile(&format!("var prototype = {{rank:42}}; var target = new Proxy(function Target() {{}}, {{get(object, key, receiver) {{if (key === 'prototype') {{$262.gc(); {action}}} return Reflect.get(object, key, receiver);}}}});"), "dynamic-constructor-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let target_atom = vm.intern_atom("target");
                let target = vm.own_property(vm.realm.globals, target_atom).unwrap();
                let prototype_atom = vm.prototype_atom();
                let prototype = if intrinsic {
                    vm.realm.intrinsics.builtin_prototypes[&(vm.realm.globals, native)]
                } else {
                    vm.own_property(vm.realm.globals, prototype_atom).unwrap()
                };
                let constructor = vm.native_value(native);
                let argument = vm.heap.alloc_string("return 42".into());
                let weak_argument = vm.heap.weak_handle(argument).unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.construct_value_with_new_target(
                    &program,
                    constructor,
                    target,
                    &[argument],
                );
                assert_eq!(result.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let weak_result = result.ok().map(|result| {
                    assert!(vm.is_function(result));
                    assert_eq!(vm.object_data(result).unwrap().proto(), prototype);
                    vm.heap.weak_handle(result).unwrap()
                });
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(weak_argument).is_none());
                if let Some(result) = weak_result {
                    assert!(vm.heap.weak_value(result).is_none());
                }
            }
        }
    }
}

#[test]
fn error_constructor_and_call_scopes_release_after_coercion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::Error,
            Native::EvalError,
            Native::RangeError,
            Native::ReferenceError,
            Native::SyntaxError,
            Native::TypeError,
            Native::URIError,
            Native::SuppressedError,
        ] {
            for (setup, fails) in [
                (
                    "var message = {toString() {$262.gc(); return 'message'}}; var options = {get cause() {$262.gc(); return {rank: 7}}};",
                    false,
                ),
                (
                    "var message = {toString() {$262.gc(); throw new Error('message')}}; var options;",
                    true,
                ),
                (
                    "var message; var options = {get cause() {$262.gc(); throw new Error('cause')}};",
                    true,
                ),
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let program = compile(setup, "error-root-scope.js").unwrap();
                vm.execute(&program).unwrap();
                let message_atom = vm.intern_atom("message");
                let message = vm.own_property(vm.realm.globals, message_atom).unwrap();
                let options_atom = vm.intern_atom("options");
                let options = vm.own_property(vm.realm.globals, options_atom).unwrap();
                let args = if native == Native::SuppressedError {
                    vec![Value::UNDEFINED, Value::UNDEFINED, message]
                } else {
                    vec![message, options]
                };
                for with_new_target in [false, true] {
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = if with_new_target {
                        let new_target = vm.native_value(native);
                        vm.construct_error_with_new_target(&program, native, &args, new_target)
                    } else {
                        vm.construct_error_native(&program, native, &args)
                    };
                    assert_eq!(
                        result.is_err(),
                        fails && (native != Native::SuppressedError || !message.is_undefined())
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    if let Ok(value) = result {
                        let handle = vm.heap.weak_handle(value).unwrap();
                        vm.collect_now(&program);
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
        for fails in [false, true] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let action = if fails {
                "throw new Error('prototype')"
            } else {
                "return {}"
            };
            let program = compile(&format!("var constructor = new Proxy(function C(value) {{$262.gc(); this.answer = value.answer;}}, {{get(target, key, receiver) {{if (key === 'prototype') {{$262.gc(); {action}}} return Reflect.get(target, key, receiver);}}}});"), "construct-scope.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("constructor");
            let constructor = vm.own_property(vm.realm.globals, atom).unwrap();
            let argument = vm.object();
            vm.set_named(&program, argument, "answer", Value::number(42.0))
                .unwrap();
            let weak = vm.heap.weak_handle(argument).unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.construct_value(&program, constructor, &[argument]);
            assert_eq!(result.is_err(), fails);
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            if let Ok(result) = result {
                let atom = vm.intern_atom("answer");
                assert_eq!(vm.own_property(result, atom), Some(Value::number(42.0)));
            }
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(weak).is_none());
        }
    }
}

#[test]
fn error_new_target_roots_release_after_prototype_success_and_throw() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::Error,
            Native::EvalError,
            Native::RangeError,
            Native::ReferenceError,
            Native::SyntaxError,
            Native::TypeError,
            Native::URIError,
            Native::SuppressedError,
            Native::AggregateError,
        ] {
            for fails in [false, true] {
                let action = if fails { "throw 37" } else { "return {}" };
                let source = format!(
                    "var target = new Proxy(function Target() {{}}, {{get(t, k, r) {{if(k === 'prototype') {{$262.gc(); {action};}} return Reflect.get(t, k, r);}}}}); function coerce() {{$262.gc(); return 'message';}}"
                );
                let program = compile(&source, "error-new-target-roots.js").unwrap();
                let mut vm = Vm::new(TestHost::default().with_test262());
                vm.execute(&program).unwrap();
                let target_atom = vm.intern_atom("target");
                let target = vm.own_property(vm.realm.globals, target_atom).unwrap();
                let coerce_atom = vm.intern_atom("coerce");
                let coerce = vm.own_property(vm.realm.globals, coerce_atom).unwrap();
                let message = vm.object();
                vm.set_named(&program, message, "toString", coerce).unwrap();
                let cause = vm.object();
                let options = vm.object();
                vm.set_named(&program, options, "cause", cause).unwrap();
                let args = match native {
                    Native::AggregateError => vec![vm.new_array(vec![cause]), message, options],
                    Native::SuppressedError => vec![cause, options, message],
                    _ => vec![message, options],
                };
                let weak = args
                    .iter()
                    .map(|value| vm.heap.weak_handle(*value).unwrap())
                    .collect::<Vec<_>>();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let constructor = vm.native_value(native);
                let result =
                    vm.construct_value_with_new_target(&program, constructor, target, &args);
                assert_eq!(result.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result = result.ok().map(|value| vm.heap.weak_handle(value).unwrap());
                vm.collect_now(&program);
                assert!(
                    weak.into_iter()
                        .all(|value| vm.heap.weak_value(value).is_none())
                );
                if let Some(value) = result {
                    assert!(vm.heap.weak_value(value).is_none());
                }
            }
        }
    }
}

#[test]
fn spread_and_aggregate_roots_release_after_guest_failures() {
    let cases = [
        (
            "var source = [{rank: 1}]; var message; var options;",
            false,
            false,
        ),
        (
            "var source = {[Symbol.iterator]() {return {get next() {$262.gc(); throw new Error('next')}}}}; var message; var options;",
            true,
            true,
        ),
        (
            "var source = {[Symbol.iterator]() {return {next() {$262.gc(); throw new Error('next')}}}}; var message; var options;",
            true,
            true,
        ),
        (
            "var source = {[Symbol.iterator]() {return {next() {return {get done() {$262.gc(); throw new Error('done')}}}}}}; var message; var options;",
            true,
            true,
        ),
        (
            "var source = {[Symbol.iterator]() {return {next() {return {done: false, get value() {$262.gc(); throw new Error('value')}}}}}}; var message; var options;",
            true,
            true,
        ),
        (
            "var source = [{rank: 1}]; var message = {toString() {$262.gc(); throw new Error('message')}}; var options;",
            false,
            true,
        ),
        (
            "var source = [{rank: 1}]; var message; var options = new Proxy({}, {has() {$262.gc(); throw new Error('has')}});",
            false,
            true,
        ),
        (
            "var source = [{rank: 1}]; var message; var options = {get cause() {$262.gc(); throw new Error('cause')}};",
            false,
            true,
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (setup, spread_fails, aggregate_fails) in cases {
            for aggregate in [false, true] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let program = compile(setup, "spread-aggregate-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let mut args = Vec::new();
                for name in ["source", "message", "options", "AggregateError"] {
                    let atom = vm.intern_atom(name);
                    args.push(vm.own_property(vm.realm.globals, atom).unwrap());
                }
                let roots = vm.heap.root_count_for_test();
                let result = if aggregate {
                    vm.construct_aggregate_error(&program, &args[..3], args[3])
                } else {
                    let array = vm.new_array(Vec::new());
                    vm.spread_into_array(&program, array, args[0]).map(|()| array)
                };
                assert_eq!(
                    result.is_err(),
                    if aggregate {
                        aggregate_fails
                    } else {
                        spread_fails
                    }
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                if let Ok(value) = result {
                    let handle = vm.heap.weak_handle(value).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn typed_initialization_roots_release_after_iterator_and_conversion_errors() {
    let sources = [
        ("({length: Number.MAX_SAFE_INTEGER + 1})", true),
        (
            "({length: 2, 0: {valueOf() {$262.gc(); return '7'}}, 1: '8'})",
            false,
        ),
        (
            "({length: 2, 0: {valueOf() {$262.gc(); throw new Error('convert')}}, 1: '8'})",
            true,
        ),
        (
            "({[Symbol.iterator]() {let index = 0; return {get next() {$262.gc(); return function() {return index++ ? {done: true} : {value: {valueOf() {$262.gc(); return '7'}}, done: false}}}}}})",
            false,
        ),
        (
            "({[Symbol.iterator]() {return {next() {$262.gc(); throw new Error('next')}}}})",
            true,
        ),
        (
            "({[Symbol.iterator]() {return {next() {return {get done() {$262.gc(); throw new Error('done')}}}}}})",
            true,
        ),
        (
            "({[Symbol.iterator]() {return {next() {return {done: false, get value() {$262.gc(); throw new Error('value')}}}}}})",
            true,
        ),
        (
            "({[Symbol.iterator]() {let index = 0; return {next() {return index++ ? {done: true} : {value: {valueOf() {$262.gc(); throw new Error('convert')}}, done: false}}}}})",
            true,
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (name, kind) in [
            ("Uint8Array", TypedArrayKind::Uint8),
            ("BigInt64Array", TypedArrayKind::BigInt64),
            ("BigUint64Array", TypedArrayKind::BigUint64),
        ] {
            for (source, fails) in sources {
                for from in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let program = compile(
                        &format!("var source = {source};"),
                        "typed-initialization-roots.js",
                    )
                    .unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("source");
                    let source = vm.own_property(vm.realm.globals, atom).unwrap();
                    let atom = vm.intern_atom(name);
                    let constructor = vm.own_property(vm.realm.globals, atom).unwrap();
                    let roots = vm.heap.root_count_for_test();
                    let outcome = if from {
                        vm.array_modern_native(
                            &program,
                            Native::TypedArrayFrom,
                            constructor,
                            &[source],
                        )
                    } else {
                        vm.construct_typed_array_native(&program, &[source], kind, name)
                    };
                    assert_eq!(outcome.is_err(), fails, "{name}, from={from}, {source:?}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    if let Ok(result) = outcome {
                        let handle = vm.heap.weak_handle(result).unwrap();
                        vm.collect_now(&program);
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn typed_fill_and_assignment_roots_release_after_coercion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for constructor in ["Uint8Array", "BigInt64Array", "BigUint64Array"] {
            let element = if constructor == "Uint8Array" {
                "7"
            } else {
                "7n"
            };
            for (setup, fails) in [
                (format!("var value = {element}; var start = 0; var end = undefined;"), false),
                ("var value = {valueOf() {$262.gc(); throw new Error('value')}}; var start = 0; var end = undefined;".into(), true),
                (format!("var value = {element}; var start = {{valueOf() {{$262.gc(); throw new Error('start')}}}}; var end = undefined;"), true),
                (format!("var value = {element}; var start = 0; var end = {{valueOf() {{$262.gc(); throw new Error('end')}}}};"), true),
                (format!("var value = {element}; var start = {{valueOf() {{$262.detachArrayBuffer(source.buffer); $262.gc(); return 0}}}}; var end = undefined;"), true),
                (format!("var value = {element}; var start = 0; var end = {{valueOf() {{$262.detachArrayBuffer(source.buffer); $262.gc(); return 1}}}};"), true),
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let program = compile(&format!("var source = new {constructor}(3); {setup}"), "typed-fill-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let mut values = Vec::new();
                for name in ["source", "value", "start", "end"] {
                    let atom = vm.intern_atom(name);
                    values.push(vm.own_property(vm.realm.globals, atom).unwrap());
                }
                let roots = vm.heap.root_count_for_test();
                let result = vm.typed_array_native(&program, Native::Uint8ArrayFill, values[0], &values[1..]);
                assert_eq!(result.is_err(), fails, "{constructor}: {setup}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
            }
            for fails in [false, true] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let coercion = if fails {
                    "throw new Error('value')".to_owned()
                } else {
                    format!("return {element}")
                };
                let program = compile(&format!("var source = new {constructor}(1); var value = {{valueOf() {{$262.gc(); {coercion}}}}};"), "typed-set-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let value_atom = vm.intern_atom("value");
                let value = vm.own_property(vm.realm.globals, value_atom).unwrap();
                let roots = vm.heap.root_count_for_test();
                let result = vm.typed_array_set(&program, source, 0, value);
                assert_eq!(result.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots);
            }
        }
    }
}

#[test]
fn typed_sort_snapshot_roots_release_after_comparator_errors() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for constructor in ["Uint8Array", "BigInt64Array", "BigUint64Array"] {
            for (comparator, fails) in [
                ("undefined", false),
                (
                    "(left, right) => left < right ? -1 : left > right ? 1 : 0",
                    false,
                ),
                ("() => {throw new Error('compare')}", true),
                ("() => ({valueOf() {throw new Error('coercion')}})", true),
            ] {
                for native in [
                    Native::TypedArraySort,
                    Native::TypedArrayToSorted,
                    Native::TypedArrayToReversed,
                ] {
                    let mut vm = Vm::new(TestHost::default());
                    let values = if constructor == "Uint8Array" {
                        "[3, 1, 2]"
                    } else {
                        "[3n, 1n, 2n]"
                    };
                    let source = format!(
                        "var source = new {constructor}({values}); var comparator = {comparator};"
                    );
                    let program = compile(&source, "typed-sort-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let source_atom = vm.intern_atom("source");
                    let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                    let comparator_atom = vm.intern_atom("comparator");
                    let comparator = vm.own_property(vm.realm.globals, comparator_atom).unwrap();
                    let roots_before = vm.heap.root_count_for_test();
                    let outcome = vm.typed_array_native(&program, native, source, &[comparator]);
                    assert_eq!(
                        outcome.is_err(),
                        fails && native != Native::TypedArrayToReversed
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots_before);
                    if native != Native::TypedArraySort
                        && let Ok(result) = outcome
                    {
                        let handle = vm.heap.weak_handle(result).unwrap();
                        vm.collect_now(&program);
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn sort_snapshot_roots_release_after_guest_failures_and_writeback() {
    let cases = [
        (
            "var source = [3, 1, 2]; var comparator = (left, right) => left - right;",
            false,
            false,
        ),
        (
            "var source = null; var comparator = (left, right) => left - right;",
            true,
            true,
        ),
        (
            "var source = [3, 1, 2]; var comparator = undefined; \
          Object.defineProperty(source, '1', {get() {throw new Error('getter')}});",
            true,
            true,
        ),
        (
            "var source = [3, 1, 2]; var comparator = () => {throw new Error('compare')};",
            true,
            true,
        ),
        (
            "var source = [3, 1, 2]; var comparator = () => \
          ({valueOf() {throw new Error('coercion')}});",
            true,
            true,
        ),
        (
            "var source = [{toString() {throw new Error('string')}}, {}]; var comparator = undefined;",
            true,
            true,
        ),
        (
            "var source = [3, 1, 2]; var comparator = undefined; \
          Object.defineProperty(source, '0', {get() {return 3}, set() {throw new Error('setter')}});",
            true,
            false,
        ),
        (
            "var source = [3, , 1]; var comparator = undefined; \
          Object.defineProperty(source, '2', {configurable: false});",
            true,
            false,
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (source, sort_fails, copy_fails) in cases {
            for (native, fails) in [
                (Native::ArraySort, sort_fails),
                (Native::ArrayToSorted, copy_fails),
            ] {
                let mut vm = Vm::new(TestHost::default());
                let program = compile(source, "sort-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let comparator_atom = vm.intern_atom("comparator");
                let comparator = vm.own_property(vm.realm.globals, comparator_atom).unwrap();
                let roots_before = vm.heap.root_count_for_test();
                let outcome = vm.array_modern_native(&program, native, source, &[comparator]);
                assert_eq!(outcome.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots_before);
                if native == Native::ArrayToSorted
                    && let Ok(result) = outcome
                {
                    let handle = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn array_copy_roots_release_after_success_getters_and_coercion_errors() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (source, getters_fail, coercion_fails) in [
            ("var source = [1, 2, 3]; var index = 1;", false, false),
            (
                "var source = [1, 2, 3]; var index = 1; \
              for (var key of [0, 1, 2]) Object.defineProperty(source, key, \
              {get() {throw new Error('getter')}});",
                true,
                false,
            ),
            (
                "var source = [1, 2, 3]; var index = {valueOf() {throw new Error('coercion')}};",
                false,
                true,
            ),
        ] {
            for native in [
                Native::ArrayWith,
                Native::ArrayToReversed,
                Native::ArrayToSpliced,
            ] {
                let mut vm = Vm::new(TestHost::default());
                let program = compile(source, "copy-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let index_atom = vm.intern_atom("index");
                let index = vm.own_property(vm.realm.globals, index_atom).unwrap();
                let roots_before = vm.heap.root_count_for_test();
                let outcome =
                    vm.array_copy_native(&program, native, source, &[index, Value::number(1.0)]);
                assert_eq!(
                    outcome.is_err(),
                    getters_fail || (coercion_fails && native != Native::ArrayToReversed)
                );
                assert_eq!(vm.heap.root_count_for_test(), roots_before);
                if let Ok(result) = outcome {
                    let handle = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn reduction_roots_release_on_success_and_abrupt_completion() {
    let cases = [
        (
            "var source = [1, 2]; var callback = (accumulator, value) => ({sum: 42});",
            false,
            false,
        ),
        (
            "var source = [1, 2]; var callback = () => {throw new Error('callback')};",
            true,
            true,
        ),
        (
            "var source = [1, 2]; var callback = () => ({sum: 42}); \
          for (var index of [0, 1]) Object.defineProperty(source, index, \
          {get() {throw new Error('getter')}});",
            true,
            true,
        ),
        (
            "var source = new Proxy([1, 2], {has() {throw new Error('has')}}); \
          var callback = () => ({sum: 42});",
            true,
            true,
        ),
        (
            "var source = []; var callback = () => ({sum: 42});",
            true,
            false,
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [Native::ArrayReduce, Native::ArrayReduceRight] {
            for (source, fails_without_initial, fails_with_initial) in cases {
                for (initial, fails) in [(false, fails_without_initial), (true, fails_with_initial)]
                {
                    let mut vm = Vm::new(TestHost::default());
                    let program = compile(source, "reduce-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let source_atom = vm.intern_atom("source");
                    let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                    let callback_atom = vm.intern_atom("callback");
                    let callback = vm.own_property(vm.realm.globals, callback_atom).unwrap();
                    let mut args = vec![callback];
                    if initial {
                        args.push(Value::UNDEFINED);
                    }
                    let roots_before = vm.heap.root_count_for_test();
                    let outcome = vm.array_reduce_native(&program, native, source, &args);
                    assert_eq!(outcome.is_err(), fails);
                    assert_eq!(vm.heap.root_count_for_test(), roots_before);
                    if let Ok(result) = outcome {
                        if let Some(handle) = vm.heap.weak_handle(result) {
                            vm.collect_now(&program);
                            assert!(vm.heap.weak_value(handle).is_none());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn flattening_roots_release_after_success_and_abrupt_completion() {
    let cases = [
        (
            "var source = [1]; var mapper = value => [value];",
            false,
            false,
        ),
        (
            "var source = [1]; var mapper = value => [value]; \
          Object.defineProperty(source, '0', {get() {throw new Error('source')}});",
            true,
            true,
        ),
        (
            "var nested = [1]; Object.defineProperty(nested, '0', \
          {get() {throw new Error('nested')}}); var source = [nested]; var mapper = value => value;",
            true,
            true,
        ),
        (
            "var source = [1]; var mapper = value => [value]; \
          source.constructor = {[Symbol.species]: function() { \
          return new Proxy({}, {defineProperty() {return false}})}};",
            true,
            true,
        ),
        (
            "var source = [1]; var mapper = value => {throw new Error('mapper')};",
            false,
            true,
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (source, flat_fails, flat_map_fails) in cases {
            for (native, fails) in [
                (Native::ArrayFlat, flat_fails),
                (Native::ArrayFlatMap, flat_map_fails),
            ] {
                let mut vm = Vm::new(TestHost::default());
                let program = compile(source, "flatten-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let source_atom = vm.intern_atom("source");
                let source = vm.own_property(vm.realm.globals, source_atom).unwrap();
                let mapper_atom = vm.intern_atom("mapper");
                let mapper = vm.own_property(vm.realm.globals, mapper_atom).unwrap();
                let args = if native == Native::ArrayFlatMap {
                    vec![mapper]
                } else {
                    vec![]
                };
                let roots_before = vm.heap.root_count_for_test();
                let outcome = vm.array_flatten_native(&program, native, source, &args);
                assert_eq!(outcome.is_err(), fails);
                assert_eq!(vm.heap.root_count_for_test(), roots_before);
                if let Ok(result) = outcome {
                    let handle = vm.heap.weak_handle(result).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn flattening_uses_rooted_frames_instead_of_native_recursion() {
    const NESTING_DEPTH: usize = 4096;
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize("", "deep-flatten.js").unwrap();
    vm.initialize(&program).unwrap();
    let mut nested = Value::number(42.0);
    for _ in 0..NESTING_DEPTH {
        nested = vm.new_array(vec![nested]);
    }
    let roots_before = vm.heap.root_count_for_test();
    let result = vm
        .array_flatten_native(
            &program,
            Native::ArrayFlat,
            nested,
            &[Value::number(f64::INFINITY)],
        )
        .unwrap();
    assert_eq!(vm.array_value_at(result, 0), Value::number(42.0));
    assert_eq!(vm.heap.root_count_for_test(), roots_before);
}

#[test]
fn bound_function_observes_proxy_metadata_order_and_prototype() {
    let source = r#"
        var events = [], prototype = {};
        var target = new Proxy(function(a, b, c) { return this.value + a; }, {
            getPrototypeOf() { events.push('prototype'); return prototype; },
            getOwnPropertyDescriptor(target, key) {
                events.push('own:' + key);
                return {value: 3, configurable: true};
            },
            get(target, key) {
                events.push('get:' + key);
                return key === 'length' ? 3.9 : 'example';
            }
        });
        var bound = Function.prototype.bind.call(target, {value: 40}, 2);
        print(events.join(','));
        print(Object.getPrototypeOf(bound) === prototype);
        print(bound.length);
        print(bound.name);
        print(bound());
        print(Object.getOwnPropertyDescriptor(bound, 'length').writable);
        print(Object.getOwnPropertyDescriptor(bound, 'length').enumerable);
        print(Object.getOwnPropertyDescriptor(bound, 'length').configurable);
        var absent = new Proxy(function() {}, {
            getOwnPropertyDescriptor() { return undefined; },
            get(target, key) {
                if (key === 'length') throw new Error('length must not be read');
                return 1337;
            }
        });
        bound = Function.prototype.bind.call(absent);
        print(bound.length);
        print(bound.name);
        var plain = function() {};
        Object.setPrototypeOf(plain, null);
        bound = Function.prototype.bind.call(plain);
        print(Object.getPrototypeOf(bound) === null);
    "#;
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = compile(source, "bound-proxy-metadata.js").unwrap();
        if let Err(error) = vm.execute(&program) {
            panic!("{}", vm.format_error(&program, &error));
        }
        assert_eq!(
            output.borrow().as_slice(),
            [
                "prototype,own:length,get:length,get:name",
                "true",
                "2",
                "bound example",
                "42",
                "false",
                "false",
                "true",
                "0",
                "bound ",
                "true",
            ]
        );
    }
}

#[test]
fn bound_function_proxy_metadata_roots_survive_gc_and_abrupt_completion() {
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        for phase in ["accept", "prototype", "descriptor", "length", "name"] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                var marker = {{}};
                var target = new Proxy(function() {{}}, {{
                    getPrototypeOf() {{
                        $262.gc(); if ('{phase}' === 'prototype') throw marker;
                        return null;
                    }},
                    getOwnPropertyDescriptor() {{
                        $262.gc(); if ('{phase}' === 'descriptor') throw marker;
                        return {{value: 3, configurable: true}};
                    }},
                    get(target, key) {{
                        $262.gc(); if ('{phase}' === key) throw marker;
                        return key === 'length' ? 3 : 'rooted';
                    }}
                }});
            "#
            );
            let program = compile(&source, "bound-proxy-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("target");
            let target = vm.own_property(vm.realm.globals, atom).unwrap();
            let roots_before = vm.heap.root_count_for_test();
            let outcome = vm.bind_function(&program, target, &[]);
            assert_eq!(vm.heap.root_count_for_test(), roots_before, "{phase}");
            match outcome {
                Ok(function) => {
                    assert_eq!(phase, "accept");
                    assert!(vm.object_data(function).unwrap().proto().is_null());
                    let atom = vm.intern_atom("length");
                    assert_eq!(vm.own_property(function, atom), Some(Value::number(3.0)));
                    let handle = vm.heap.weak_handle(function).unwrap();
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(handle).is_none());
                }
                Err(error) => {
                    assert_ne!(phase, "accept");
                    let atom = vm.intern_atom("marker");
                    let marker = vm.own_property(vm.realm.globals, atom).unwrap();
                    assert_eq!(error.thrown_value(), Some(marker), "{phase}");
                }
            }
        }
    }
}

#[test]
fn bound_function_metadata_roots_release_on_normal_and_abrupt_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (metadata, fails) in [
            (None, false),
            (Some("length"), true),
            (Some("name"), true),
            (Some("getPrototypeOf"), true),
            (Some("getOwnPropertyDescriptor"), true),
        ] {
            let mut vm = Vm::new(TestHost::default());
            let source = match metadata {
                None => "function target() {}".to_owned(),
                Some(trap @ ("getPrototypeOf" | "getOwnPropertyDescriptor")) => format!(
                    "var target = new Proxy(function() {{}}, {{ \
                     {trap}() {{throw new Error('{trap}')}} }});"
                ),
                Some(property) => format!(
                    "function target() {{}} Object.defineProperty(target, '{property}', \
                     {{get() {{throw new Error('{property}')}}}});"
                ),
            };
            let program = compile(&source, "bound-metadata-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("target");
            let target = vm.own_property(vm.realm.globals, atom).unwrap();
            let roots_before = vm.heap.root_count_for_test();
            let outcome = vm.bind_function(&program, target, &[]);
            assert_eq!(outcome.is_err(), fails, "{metadata:?}");
            assert_eq!(vm.heap.root_count_for_test(), roots_before, "{metadata:?}");
            if let Ok(function) = outcome {
                let handle = vm.heap.weak_handle(function).unwrap();
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(handle).is_none());
            }
        }
    }
}

#[test]
fn closure_identity_cache_prunes_collected_cells_and_keeps_rooted_cells() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize("function kept() {}", "closure-cache-gc.js").unwrap();
    vm.initialize(&program).unwrap();
    let dead = vm.closure(&program, 0, Value::NULL).unwrap();
    let kept = vm.closure(&program, 1, Value::NULL).unwrap();
    let root = vm.root(kept);
    vm.collect_now(&program);
    assert!(!vm.heap.is_live(dead));
    assert!(vm.heap.is_live(kept));
    assert!(
        !vm.function_values
            .contains_key(&(super::program_store::ProgramId::MAIN, 0))
    );
    assert!(
        vm.function_values
            .contains_key(&(super::program_store::ProgramId::MAIN, 1))
    );
    assert!(vm.release_root(root));
    vm.collect_now(&program);
    assert!(
        !vm.function_values
            .contains_key(&(super::program_store::ProgramId::MAIN, 1))
    );
}

#[test]
fn closure_identity_cache_rejects_reused_function_slots() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize("function kept() {}", "closure-cache-reuse.js").unwrap();
    vm.initialize(&program).unwrap();
    let old = vm.closure(&program, 1, Value::NULL).unwrap();
    let key = (super::program_store::ProgramId::MAIN, 1);
    let allocation_bound = vm.heap.stats().2 + 1;
    vm.collect_now(&program);
    assert!(!vm.function_values.contains_key(&key));
    let mut reused = false;
    for _ in 0..allocation_bound {
        let replacement = vm.heap.alloc_function(crate::heap::FunctionRecord {
            object: Vm::<TestHost>::empty_object(Value::NULL),
            realm: Value::NULL,
            kind: crate::heap::FunctionKind::User(key.0, key.1),
            env: Value::NULL,
        });
        if replacement == old {
            reused = true;
            break;
        }
    }
    assert!(reused, "the collected function slot must be reused");
    assert!(
        vm.cached_functions_in_environment(key.0, key.1, Value::NULL)
            .next()
            .is_none()
    );
    vm.prune_function_values();
    assert!(!vm.function_values.contains_key(&key));
}

#[test]
fn finalization_unregister_does_not_match_a_reused_dead_token_slot() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize("", "finalization-token-reuse.js").unwrap();
    let target = vm
        .heap
        .alloc(crate::heap::Cell::Object(Vm::<TestHost>::empty_object(
            Value::NULL,
        )));
    let token = vm
        .heap
        .alloc(crate::heap::Cell::Object(Vm::<TestHost>::empty_object(
            Value::NULL,
        )));
    let target_handle = vm.heap.weak_handle(target).unwrap();
    let token_handle = vm.heap.weak_handle(token).unwrap();
    let registry = vm.heap.alloc(crate::heap::Cell::FinalizationRegistry {
        object: Box::new(Vm::<TestHost>::empty_object(Value::NULL)),
        callback: Value::UNDEFINED,
        entries: Box::new(crate::heap::FinalizationEntries(vec![
            crate::heap::FinalizationEntry {
                target: target_handle,
                held: Value::number(1.0),
                token: Some(token_handle),
            },
        ])),
    });
    let target_root = vm.heap.root(target);
    let registry_root = vm.heap.root(registry);

    assert!(vm.heap.collect([target, registry]).is_empty());
    assert!(vm.heap.is_live(target));
    let crate::heap::Cell::FinalizationRegistry { entries, .. } = vm.heap.get(registry).unwrap()
    else {
        panic!("registry remains live");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].token, None);

    let allocation_bound = vm.heap.stats().2 + 1;
    let replacement = (0..allocation_bound)
        .map(|_| {
            vm.heap
                .alloc(crate::heap::Cell::Object(Vm::<TestHost>::empty_object(
                    Value::NULL,
                )))
        })
        .find(|value| *value == token)
        .expect("freed token slot is reused");
    assert_eq!(
        vm.call_finalization_registry_native(
            &program,
            Native::FinalizationRegistryUnregister,
            registry,
            &[replacement],
        )
        .unwrap(),
        Value::FALSE
    );
    assert!(vm.heap.release_root(target_root));
    assert!(vm.heap.release_root(registry_root));
}

#[test]
fn pending_jobs_use_the_shared_interpreter_after_root_release() {
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize("print(0);", "job.js").unwrap();
    vm.initialize(&program).unwrap();
    let callback = vm.native_value(crate::heap::Native::Print);
    let callback_root = vm.root(callback);
    let argument_root = vm.root(Value::number(7.0));
    let argument = vm.root_value(argument_root).unwrap();
    vm.enqueue_job(callback, vec![argument]);
    assert!(vm.release_root(callback_root));
    assert!(vm.release_root(argument_root));
    vm.drain_jobs(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["7"]);
}

#[test]
fn internal_promise_then_handlers_survive_species_reentry_and_release_after_jobs() {
    let mut vm = Vm::new(TestHost::default().with_test262());
    let program = Engine::specialize(
        "var promise = Promise.resolve(1); Object.defineProperty(promise, 'constructor', {get() {$262.gc(); return Promise;}});",
        "internal-then-owner.js",
    ).unwrap();
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("promise");
    let promise = vm.own_property(vm.realm.globals, name).unwrap();
    let held = vm
        .heap
        .alloc(crate::heap::Cell::Error("handler state".into()));
    let weak = vm.heap.weak_handle(held).unwrap();
    let handler = vm.native_with_env(Native::Print, held);
    vm.promise_then(&program, promise, handler, Value::UNDEFINED)
        .unwrap();
    assert_eq!(vm.heap.weak_value(weak), Some(held));
    assert!(vm.active_call_roots.is_empty());
    vm.drain_jobs(&program).unwrap();
    vm.collect_now(&program);
    assert!(vm.heap.weak_value(weak).is_none());
}

#[test]
fn dynamic_import_waiters_are_rooted_while_queued_and_during_batch_evaluation() {
    use super::promise::{DynamicImportJob, PromiseState};
    for collect_while_queued in [false, true] {
        let mut vm = Vm::new(TestHost::default().with_test262());
        let program = Engine::specialize("", "dynamic-import-owner.js").unwrap();
        vm.execute(&program).unwrap();
        let mut promises = Vec::new();
        for index in 0..2 {
            let promise = vm.promise_object().unwrap();
            promises.push((promise, vm.heap.weak_handle(promise).unwrap()));
            let name = format!("dynamic-job-{index}.js");
            let source = "$262.gc(); export const value = 57;".to_owned();
            vm.realm.promise.dynamic_import_jobs.push(DynamicImportJob {
                cache_key: format!("{name}:javascript"),
                module: crate::ModuleSource::text(name, source),
                promise,
            });
        }
        if collect_while_queued {
            vm.collect_now(&program);
            for (promise, weak) in &promises {
                assert_eq!(vm.heap.weak_value(*weak), Some(*promise));
            }
            vm.advance_dynamic_import_jobs(&program, false).unwrap();
            assert_eq!(vm.realm.promise.dynamic_import_jobs.len(), promises.len());
        }
        vm.advance_dynamic_import_jobs(&program, true).unwrap();
        assert!(vm.realm.promise.dynamic_import_jobs.is_empty());
        for (promise, weak) in &promises {
            assert_eq!(vm.heap.weak_value(*weak), Some(*promise));
            assert_eq!(
                vm.promise_record(*promise).expect("promise record").state,
                PromiseState::Fulfilled
            );
        }
        vm.collect_now(&program);
        for (promise, weak) in promises {
            assert!(vm.heap.weak_value(weak).is_none());
            assert!(!vm.heap.is_live(promise));
        }
    }
}

#[test]
fn async_continuation_lifetime_follows_reachable_callbacks_and_jobs() {
    for queued in [false, true] {
        let mut vm = Vm::new(TestHost::default());
        let program = Engine::specialize(
            "function make() { var gate = Promise.withResolvers(), held = {value:57}; async function run() { let retained = held; await gate.promise; return retained; } return {gate:gate, result:run(), held:held}; }",
            "async-continuation-owner.js",
        ).unwrap();
        vm.execute(&program).unwrap();
        let name = vm.intern_atom("make");
        let factory = vm.own_property(vm.realm.globals, name).unwrap();
        let values = vm
            .call_value(&program, factory, Value::UNDEFINED, &[])
            .unwrap();
        let held_atom = vm.intern_atom("held");
        let held = vm.own_property(values, held_atom).unwrap();
        let held_weak = vm.heap.weak_handle(held).unwrap();
        assert_eq!(pending_await_continuations(&mut vm), 1);
        let root = vm.heap.root(values);
        vm.collect_now(&program);
        assert_eq!(vm.heap.weak_value(held_weak), Some(held));
        if queued {
            let gate_atom = vm.intern_atom("gate");
            let gate = vm.own_property(values, gate_atom).unwrap();
            let resolve_atom = vm.intern_atom("resolve");
            let resolve = vm.own_property(gate, resolve_atom).unwrap();
            vm.call_value(&program, resolve, Value::UNDEFINED, &[])
                .unwrap();
            assert!(!vm.realm.jobs.is_empty());
        }
        assert!(vm.heap.release_root(root));
        vm.collect_now(&program);
        if queued {
            assert_eq!(vm.heap.weak_value(held_weak), Some(held));
            vm.drain_jobs(&program).unwrap();
            vm.collect_now(&program);
        }
        assert!(vm.heap.weak_value(held_weak).is_none(), "queued={queued}");
        assert_eq!(pending_await_continuations(&mut vm), 0, "queued={queued}");
    }
}

fn pending_await_continuations(vm: &mut Vm<TestHost>) -> usize {
    let mut pending = 0;
    vm.heap
        .for_each_await_state(|state| pending += usize::from(state.continuation.is_some()));
    pending
}

#[test]
fn await_state_traces_its_continuation_until_a_reaction_takes_it() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize("print(0);", "continuation.js").unwrap();
    vm.initialize(&program).unwrap();
    let live = vm.heap.alloc(crate::heap::Cell::Environment {
        parent: Value::NULL,
        function: u32::MAX,
        slots: Vec::<Value>::new().into_boxed_slice().into(),
        scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
            program: None,
            root_eval_scope: false,
            binding_site_pc: None,
            dynamic_bindings: vec![].into(),
            with_objects: Box::default(),
        }),
    });
    let held = vm
        .heap
        .alloc(crate::heap::Cell::Error("suspended binding".into()));
    let original = vm
        .heap
        .alloc(crate::heap::Cell::Error("suspended argument".into()));
    let scope = vm
        .heap
        .alloc(crate::heap::Cell::Error("suspended scope".into()));
    let held_atom = vm.intern_atom("held");
    let mut frame = Box::new(super::Frame::empty(super::program_store::ProgramId::MAIN, 0));
    frame.original_arguments = vec![original];
    frame.with_objects = vec![scope];
    frame.env = live;
    frame.dynamic_bindings = vec![(held_atom, held)];
    let continuation =
        Continuation::suspend(frame, Completion::Yield(Value::UNDEFINED), None, Value::UNDEFINED);
    let state = vm.heap.alloc(crate::heap::Cell::AwaitState(Box::new(
        crate::heap::AwaitState {
            continuation: Some(Box::new(continuation)),
            realm: Value::UNDEFINED,
            promise: Value::UNDEFINED,
            generator: None,
            yielded: false,
        },
    )));
    let root = vm.heap.root(state);
    vm.collect_now(&program);
    assert!(vm.heap.is_live(live));
    assert!(vm.heap.is_live(held));
    assert!(vm.heap.is_live(original));
    assert!(vm.heap.is_live(scope));
    let take = |vm: &mut Vm<TestHost>| match vm.heap.await_state_mut(state) {
        Some(state) => state.continuation.take(),
        _ => panic!("await state"),
    };
    assert_eq!(take(&mut vm).unwrap().frame.original_arguments, [original]);
    assert!(take(&mut vm).is_none());
    vm.collect_now(&program);
    assert!(!vm.heap.is_live(live));
    assert!(!vm.heap.is_live(held));
    assert!(!vm.heap.is_live(original));
    assert!(!vm.heap.is_live(scope));
    assert!(vm.heap.is_live(state));
    assert!(vm.heap.release_root(root));
}

#[test]
fn pooled_frame_registers_are_reset_when_their_length_is_reused() {
    let mut frame = super::Frame {
        context: super::activation::CallContext::Internal,
        reflected_parameters: 0,
        original_arguments: vec![Value::heap(14)],
        program: super::program_store::ProgramId::MAIN,
        function: 0,
        pc: 0,
        pc_is_resume: true,
        js_jit_skip_entry_once: false,
        source_field: None,
        binding_site_pc: None,
        env: Value::NULL,
        this: Value::UNDEFINED,
        locals: vec![],
        dynamic_bindings: vec![],
        captured: false,
        registers: vec![Value::heap(11), Value::heap(12)],
        initialized_registers: 0,
        capture_cache: super::CaptureCache::default(),
        active_iterators: vec![],
        with_objects: vec![Value::heap(16)],
        with_base: 0,
        js_jit_interpreter_started: None,
        fixed_this: false,
    };

    frame.prepare_registers(1, crate::bytecode::InitialRegister::Undefined);
    assert_eq!(frame.registers, [Value::UNDEFINED]);

    frame.registers[0] = Value::heap(13);
    frame.prepare_registers(3, crate::bytecode::InitialRegister::Undefined);
    assert_eq!(
        frame.registers,
        [Value::UNDEFINED, Value::UNDEFINED, Value::UNDEFINED]
    );
    frame.registers[1] = Value::heap(13);
    frame.prepare_registers(2, crate::bytecode::InitialRegister::I32Zero);
    assert_eq!(frame.registers, [Value::integer(0), Value::integer(0)]);
    assert_eq!(frame.original_arguments, [Value::heap(14)]);
    frame.context = super::activation::CallContext::Function(Value::heap(15));
    let mut recycled = frame;
    Vm::<TestHost>::recycle_frame(&mut recycled);
    assert_eq!(recycled.context, super::activation::CallContext::Internal);
    assert!(recycled.original_arguments.is_empty());
    assert!(recycled.with_objects.is_empty());
}

#[test]
fn wasm_retirement_invalidates_register_reuse_after_shrinking() {
    let mut vm = Vm::new(TestHost::default());
    let mut frame = Box::new(super::Frame::empty(
        super::program_store::ProgramId::MAIN,
        0,
    ));
    frame.prepare_registers(u16::MAX, crate::bytecode::InitialRegister::I32Zero);
    let prior_capacity = frame.registers.capacity();
    vm.frames.push(frame);
    vm.retire_wasm_frame();
    let mut frame = vm.pooled_frame();
    assert!(frame.registers.capacity() < prior_capacity);
    assert_eq!(frame.initialized_registers, 0);

    // Exercise register preparation only: a mapped JS activation must grow and
    // initialize the retired buffer, rather than set its length past capacity.
    let mut program =
        Engine::specialize("function identity(x) { return x; }", "retired-frame.js").unwrap();
    program.compile_released();
    let function = &mut program.functions[1];
    function.registers = u16::MAX;
    function.register_roots = crate::bytecode::RegisterRoots::new(true);
    frame.prepare_registers_for(function);
    assert_eq!(frame.registers.len(), usize::from(u16::MAX));
    assert!(frame.registers.capacity() >= frame.registers.len());
    assert!(
        frame
            .registers
            .iter()
            .all(|value| *value == Value::UNDEFINED)
    );
}

#[test]
fn regression_collection_preserves_unused_regexp_iterator_prototype() {
    for (mode, compile) in [
        ("specialized", Engine::specialize as fn(&str, &str) -> _),
        ("unspecialized", Engine::specialize_unspecialized),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        let program = compile(
            "function later() { print((/a/g)[Symbol.matchAll]('a').next().value[0]); }",
            "regexp-after-collection.js",
        )
        .unwrap();
        vm.execute(&program).unwrap();
        let atom = vm.intern_atom("later");
        let callback = vm.own_property(vm.realm.globals, atom).unwrap();
        let root = vm.root(callback);
        vm.collect_now(&program);
        vm.enqueue_job(vm.root_value(root).unwrap(), vec![]);
        if let Err(error) = vm.drain_jobs(&program) {
            panic!("{mode}: {}", vm.format_error(&program, &error));
        }
        assert_eq!(output.borrow().as_slice(), ["a"], "{mode}");
        assert!(vm.release_root(root));
    }
}

#[test]
fn error_data_survives_collection_without_retaining_dead_errors() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in [
            Native::Error,
            Native::TypeError,
            Native::AggregateError,
            Native::SuppressedError,
        ] {
            let mut vm = Vm::new(TestHost::default());
            let program = compile("", "error-data-roots.js").unwrap();
            vm.initialize(&program).unwrap();
            let roots = vm.heap.root_count_for_test();
            let error = vm.construct_error_native(&program, kind, &[]).unwrap();
            assert!(vm.error_is_error(error));
            let root = vm.heap.root(error);
            assert!(matches!(
                vm.embedding_builtin_data(root),
                Ok(Some(crate::BuiltinData::Error))
            ));
            let weak = vm.heap.weak_handle(error).unwrap();
            vm.collect_now(&program);
            let error = vm.heap.root_value(root).unwrap();
            assert!(vm.error_is_error(error));
            assert!(matches!(
                vm.embedding_builtin_data(root),
                Ok(Some(crate::BuiltinData::Error))
            ));
            let clone = vm
                .heap
                .alloc(super::Cell::Object(vm.object_data(error).unwrap().clone()));
            assert!(vm.error_is_error(clone));
            vm.heap.release_root(root);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(weak).is_none());
            assert_eq!(vm.heap.root_count_for_test(), roots);
            let plain = vm.object();
            assert!(!vm.error_is_error(plain));
        }
        let mut vm = Vm::new(TestHost::default());
        let program = compile("", "error-data-internal.js").unwrap();
        vm.initialize(&program).unwrap();
        let exhausted = vm.stack_exhaustion_error().thrown_value().unwrap();
        assert!(vm.error_is_error(exhausted));
        let aggregate = vm.aggregate_error(&program, vec![]).unwrap();
        assert!(vm.error_is_error(aggregate));
    }
}

#[test]
fn aggregate_error_intrinsics_and_reason_roots_follow_the_active_realm() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for foreign in [false, true] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let program = compile(
                "var foreign = $262.createRealm().global;",
                "aggregate-intrinsics-roots.js",
            )
            .unwrap();
            vm.execute(&program).unwrap();
            if foreign {
                let atom = vm.intern_atom("foreign");
                let global = vm.own_property(vm.realm.globals, atom).unwrap();
                vm.switch_realm_global(global);
            }
            let global = vm.realm.globals;
            let error_prototype =
                vm.realm.intrinsics.builtin_prototypes[&(global, Native::AggregateError)];
            let array_prototype = vm.array_prototype_for_realm(global);
            let promise_prototype =
                vm.realm.intrinsics.builtin_prototypes[&(global, Native::Promise)];
            for name in ["AggregateError", "Array", "Promise"] {
                let atom = vm.intern_atom(name);
                vm.set_property(global, atom, Value::NULL).unwrap();
            }
            vm.collect_now(&program);
            assert!(vm.heap.is_live(promise_prototype));
            let roots = vm.heap.root_count_for_test();
            let reason = vm.object();
            let reason_weak = vm.heap.weak_handle(reason).unwrap();
            let error = vm.aggregate_error(&program, vec![reason]).unwrap();
            assert!(vm.error_is_error(error));
            assert_eq!(vm.object_data(error).unwrap().proto(), error_prototype);
            let errors_atom = vm.intern_atom("errors");
            let errors = vm.own_property(error, errors_atom).unwrap();
            assert_eq!(vm.object_data(errors).unwrap().proto(), array_prototype);
            let error_weak = vm.heap.weak_handle(error).unwrap();
            let errors_weak = vm.heap.weak_handle(errors).unwrap();
            let root = vm.heap.root(error);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(reason_weak).is_some());
            assert!(vm.heap.weak_value(errors_weak).is_some());
            vm.heap.release_root(root);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(reason_weak).is_none());
            assert!(vm.heap.weak_value(errors_weak).is_none());
            assert!(vm.heap.weak_value(error_weak).is_none());
            assert_eq!(vm.heap.root_count_for_test(), roots);
        }
    }
}

#[test]
fn promise_finally_restores_roots_after_each_effect_completion() {
    let cases = [
        (
            "({get constructor() {$262.gc(); throw 'constructor';}})",
            true,
        ),
        ("({constructor: {[Symbol.species]: () => {}}})", true),
        (
            "({constructor: undefined, get then() {$262.gc(); throw 'then';}})",
            true,
        ),
        (
            "({constructor: undefined, get then() {$262.gc(); return function() {$262.gc(); throw 'call';};}})",
            true,
        ),
        (
            "({constructor: undefined, get then() {$262.gc(); return function(a,b) {$262.gc(); return a === b ? 42 : 0;};}})",
            false,
        ),
    ];
    let callable_case = "({constructor: undefined, get then() {$262.gc(); throw 'then';}})";
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (source, fails, callable) in cases
            .into_iter()
            .map(|(source, fails)| (source, fails, false))
            .chain(std::iter::once((callable_case, true, true)))
        {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let program = compile(
                &format!("function create() {{return {source};}} function createHandler() {{return function() {{}};}}"),
                "finally-roots.js",
            )
            .unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("create");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let receiver = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let handler = if callable {
                let atom = vm.intern_atom("createHandler");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                vm.call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap()
            } else {
                vm.object()
            };
            let receiver_weak = vm.heap.weak_handle(receiver).unwrap();
            let handler_weak = vm.heap.weak_handle(handler).unwrap();
            let roots = vm.heap.root_count_for_test();
            let incoming = vm.active_call_roots.len();
            let result = vm.call_native(&program, Native::PromiseFinally, receiver, &[handler]);
            assert_eq!(result.is_err(), fails, "{source}");
            if !fails {
                assert_eq!(result.unwrap(), Value::number(42.0));
            }
            assert_eq!(vm.heap.root_count_for_test(), roots, "{source}");
            assert_eq!(vm.active_call_roots.len(), incoming, "{source}");
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(receiver_weak).is_none(), "{source}");
            assert!(vm.heap.weak_value(handler_weak).is_none(), "{source}");
        }
    }
}

#[test]
fn promise_constructor_intrinsics_survive_guest_binding_and_prototype_mutation() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let mut vm = Vm::new(TestHost::default().with_test262());
        let program = compile(
            "var foreign = $262.createRealm().global; var invalid = () => {};",
            "promise-intrinsic-roots.js",
        )
        .unwrap();
        vm.execute(&program).unwrap();
        let invalid_atom = vm.intern_atom("invalid");
        let invalid = vm.own_property(vm.realm.globals, invalid_atom).unwrap();
        let foreign_atom = vm.intern_atom("foreign");
        let foreign = vm.own_property(vm.realm.globals, foreign_atom).unwrap();
        vm.switch_realm_global(foreign);
        let constructor = vm.realm.intrinsics.promise_constructors[&foreign];
        let weak = vm.heap.weak_handle(constructor).unwrap();
        let prototype = vm.realm.intrinsics.builtin_prototypes[&(foreign, Native::Promise)];
        let promise_atom = vm.intern_atom("Promise");
        vm.set_property(foreign, promise_atom, Value::NULL).unwrap();
        let constructor_atom = vm.intern_atom("constructor");
        vm.set_property(prototype, constructor_atom, Value::NULL)
            .unwrap();
        vm.collect_now(&program);
        assert_eq!(vm.heap.weak_value(weak), Some(constructor));
        let source = vm.promise_object().unwrap();
        vm.set_property(source, constructor_atom, Value::UNDEFINED)
            .unwrap();
        let roots = vm.heap.root_count_for_test();
        let result = vm
            .promise_then(&program, source, Value::UNDEFINED, Value::UNDEFINED)
            .unwrap();
        assert_eq!(vm.object_data(result).unwrap().proto(), prototype);
        assert_eq!(vm.heap.root_count_for_test(), roots);
        let value = vm.object();
        assert!(
            vm.promise_resolve_for_constructor(&program, invalid, value)
                .is_err()
        );
        assert_eq!(vm.heap.root_count_for_test(), roots);
    }
}

#[test]
fn aggregate_native_roots_restore_after_reject_callbacks_throw() {
    use super::promise::AggregateMode;
    let cases = [
        (
            AggregateMode::All,
            "get resolve() {$262.gc(); throw 'resolve';}",
            "[]",
        ),
        (
            AggregateMode::All,
            "get resolve() {return value => value;}",
            "({get [Symbol.iterator]() {$262.gc(); throw 'iterator';}})",
        ),
        (
            AggregateMode::All,
            "get resolve() {return value => value;}",
            "({[Symbol.iterator]() {return {next() {$262.gc(); throw 'next';}};}})",
        ),
        (
            AggregateMode::All,
            "get resolve() {return value => value;}",
            "({[Symbol.iterator]() {return {next() {return {get done() {$262.gc(); throw 'done';}};}};}})",
        ),
        (
            AggregateMode::All,
            "get resolve() {return value => value;}",
            "({[Symbol.iterator]() {return {next() {return {done:false, get value() {$262.gc(); throw 'value';}};}};}})",
        ),
        (
            AggregateMode::All,
            "get resolve() {return function() {$262.gc(); throw {rank:42};};}",
            "({[Symbol.iterator]() {return {next() {return {done:false,value:1};}, return() {$262.gc(); throw 'close';}};}})",
        ),
        (
            AggregateMode::AllSettled,
            "get resolve() {return function() {return {get then() {$262.gc(); throw {rank:42};}};};}",
            "[1]",
        ),
        (
            AggregateMode::Any,
            "get resolve() {return function() {return {then() {$262.gc(); throw {rank:42};}};};}",
            "[1]",
        ),
        (
            AggregateMode::AllKeyed,
            "get resolve() {return value => value;}",
            "new Proxy({}, {ownKeys() {return ['entry'];}, getOwnPropertyDescriptor() {$262.gc(); throw 'descriptor';}})",
        ),
        (
            AggregateMode::AllSettledKeyed,
            "get resolve() {return value => value;}",
            "({get first() {return {rank:42};}, get last() {$262.gc(); throw 'read';}})",
        ),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (mode, resolve, input) in cases {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let setup = format!(
                "function Constructor(executor) {{executor(function() {{}}, function() {{$262.gc(); throw 'reject';}}); return {{}};}} Object.defineProperty(Constructor, 'resolve', Object.getOwnPropertyDescriptor({{{resolve}}}, 'resolve')); function create() {{return {input};}}"
            );
            let program = compile(&setup, "aggregate-root-errors.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("Constructor");
            let constructor = vm.own_property(vm.realm.globals, atom).unwrap();
            let atom = vm.intern_atom("create");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let input = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let roots = vm.heap.root_count_for_test();
            let incoming = vm.active_call_roots.len();
            assert!(
                vm.promise_aggregate(&program, constructor, &[input], mode)
                    .is_err(),
                "{setup}"
            );
            assert_eq!(vm.heap.root_count_for_test(), roots, "{setup}");
            assert_eq!(vm.active_call_roots.len(), incoming, "{setup}");
        }
    }
}

#[test]
fn property_copy_roots_release_after_callback_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for create in [false, true] {
            for phase in [
                "success",
                "keys",
                "descriptor",
                "get",
                "write",
                "reject",
                "excluded",
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function operands() {{
                        var source=new Proxy({{first:1,later:2}}, {{
                            ownKeys(object) {{$262.gc(); if ('{phase}'==='keys') throw {{kind:'keys'}};return Reflect.ownKeys(object);}},
                            getOwnPropertyDescriptor(object,key) {{$262.gc();if (('{phase}'==='descriptor' && key==='later') || ('{phase}'==='excluded' && key==='first')) throw {{kind:'descriptor'}};return Reflect.getOwnPropertyDescriptor(object,key);}},
                            get(object,key) {{$262.gc();if ('{phase}'==='get' && key==='later') throw {{kind:'get'}};return {{rank:key==='first'?42:43}};}}
                        }});
                        var target=new Proxy({{}}, {{
                            set(object,key,value) {{$262.gc();if ('{phase}'==='write' && key==='later') throw {{kind:'write'}};if ('{phase}'==='reject' && key==='later') return false;return Reflect.set(object,key,value);}},
                            defineProperty(object,key,descriptor) {{$262.gc();if ('{phase}'==='write' && key==='later') throw {{kind:'write'}};if ('{phase}'==='reject' && key==='later') return false;return Reflect.defineProperty(object,key,descriptor);}}
                        }});
                        return [target,source];
                    }}
                "#
                );
                let program = compile(&source, "copy-root-scope.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let args = vm
                    .heap
                    .array_elements(operands)
                    .map(|elements| elements.to_vec())
                    .expect("operands");
                let handles = args
                    .iter()
                    .map(|v| vm.heap.weak_handle(*v).unwrap())
                    .collect::<Vec<_>>();
                let excluded = if phase == "excluded" {
                    vec![vm.heap.alloc_string("first".into())]
                } else {
                    vec![]
                };
                let first_atom = vm.intern_atom("first");
                let later_atom = vm.intern_atom("later");
                let rank_atom = vm.intern_atom("rank");
                let excluded_handles = excluded
                    .iter()
                    .map(|value| vm.heap.weak_handle(*value).unwrap())
                    .collect::<Vec<_>>();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let kind = if create {
                    super::object_symbols::PropertyCopyKind::CreateDataProperty
                } else {
                    super::object_symbols::PropertyCopyKind::Set
                };
                let result =
                    vm.copy_enumerable_properties(&program, args[0], args[1], &excluded, kind);
                assert_eq!(
                    result.is_ok(),
                    matches!(phase, "success" | "excluded"),
                    "create={create} {phase}"
                );
                if let Err(error) = &result {
                    if phase == "reject" {
                        assert!(vm.format_error(&program, error).contains("TypeError"));
                    } else {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    }
                }
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                if phase != "keys" && phase != "excluded" {
                    let value = vm.get_property(&program, args[0], first_atom).unwrap();
                    let rank = vm.get_property(&program, value, rank_atom).unwrap();
                    assert_eq!(rank, Value::number(42.0));
                }
                if phase == "success" || phase == "excluded" {
                    let value = vm.get_property(&program, args[0], later_atom).unwrap();
                    assert_eq!(
                        vm.get_property(&program, value, rank_atom).unwrap(),
                        Value::number(43.0)
                    );
                }
                vm.collect_now(&program);
                for handle in handles.into_iter().chain(excluded_handles) {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn descriptor_snapshot_and_assign_root_fresh_native_arguments() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for descriptors in [false, true] {
            for fails in [false, true] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function operands() {{return [1,new Proxy({{first:1,later:2}},{{
                        getOwnPropertyDescriptor(object,key) {{$262.gc();if ({fails} && key==='later') throw {{kind:'later'}};return {{value:{{rank:key==='first'?42:43}},writable:true,enumerable:true,configurable:true}};}},
                        get(object,key) {{$262.gc();if ({fails} && key==='later') throw {{kind:'later'}};return {{rank:key==='first'?42:43}};}}
                    }}),{{late:47}}];}}
                "#
                );
                let program = compile(&source, "snapshot-native-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let args = vm
                    .heap
                    .array_elements(operands)
                    .map(|elements| elements.to_vec())
                    .expect("operands");
                let source_handle = vm.heap.weak_handle(args[1]).unwrap();
                let later_handle = vm.heap.weak_handle(args[2]).unwrap();
                let first_atom = vm.intern_atom("first");
                let rank_atom = vm.intern_atom("rank");
                let value_atom = vm.intern_atom("value");
                let late_atom = vm.intern_atom("late");
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if descriptors {
                    vm.object_get_own_property_descriptors(&program, &args[1..2])
                } else {
                    vm.object_assign(&program, &args)
                };
                assert_eq!(result.is_err(), fails);
                if let Err(error) = &result {
                    let atom = vm.intern_atom("kind");
                    let kind = vm
                        .own_property(error.thrown_value().unwrap(), atom)
                        .unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), "later");
                }
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = if let Ok(result) = result {
                    let first = vm.get_property(&program, result, first_atom).unwrap();
                    let first = if descriptors {
                        vm.get_property(&program, first, value_atom).unwrap()
                    } else {
                        first
                    };
                    assert_eq!(
                        vm.get_property(&program, first, rank_atom).unwrap(),
                        Value::number(42.0)
                    );
                    if !descriptors {
                        assert_eq!(
                            vm.get_property(&program, result, late_atom).unwrap(),
                            Value::number(47.0)
                        );
                    }
                    Some(vm.heap.weak_handle(result).unwrap())
                } else {
                    None
                };
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(source_handle).is_none());
                assert!(vm.heap.weak_value(later_handle).is_none());
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn for_in_roots_release_after_prototype_and_key_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for query in [false, true] {
            for phase in [
                "success",
                "keys-getter",
                "keys",
                "descriptor-getter",
                "descriptor",
                "prototype-getter",
                "prototype",
                "cycle",
                "shadow",
                "missing",
            ] {
                if query && matches!(phase, "keys-getter" | "keys") {
                    continue;
                }
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function operand() {{
                        var object={{first:1,later:2}};
                        Object.defineProperty(object,'hidden',{{value:4,enumerable:false,configurable:true}});
                        var source=new Proxy(object,{{
                            get ownKeys() {{$262.gc();if ('{phase}'==='keys-getter') throw {{kind:'keys-getter'}};
                                return function(object) {{$262.gc();if ('{phase}'==='keys') throw {{kind:'keys'}};return Reflect.ownKeys(object);}};
                            }},
                            get getOwnPropertyDescriptor() {{$262.gc();if ('{phase}'==='descriptor-getter') throw {{kind:'descriptor-getter'}};
                                return function(object,key) {{$262.gc();if ('{phase}'==='descriptor') throw {{kind:'descriptor'}};return Reflect.getOwnPropertyDescriptor(object,key);}};
                            }},
                            get getPrototypeOf() {{$262.gc();if ('{phase}'==='prototype-getter') throw {{kind:'prototype-getter'}};
                                return function() {{$262.gc();if ('{phase}'==='prototype') throw {{kind:'prototype'}};
                                    if ('{phase}'==='cycle') return source;
                                    return new Proxy({{inherited:3,hidden:5}},{{
                                        ownKeys(object) {{$262.gc();return Reflect.ownKeys(object);}},
                                        getOwnPropertyDescriptor(object,key) {{$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);}},
                                        getPrototypeOf() {{$262.gc();return null;}}
                                    }});
                                }};
                            }}
                        }});
                        return source;
                    }}
                "#
                );
                let program = compile(&source, "for-in-native-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operand");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let source = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let source_handle = vm.heap.weak_handle(source).unwrap();
                let key = vm.heap.alloc_string(
                    match phase {
                        "shadow" => "hidden",
                        "missing" => "absent",
                        _ => "inherited",
                    }
                    .into(),
                );
                let key_handle = vm.heap.weak_handle(key).unwrap();
                // Snapshot collection has no key input to keep alive.
                let key_root = (!query).then(|| vm.heap.root(key));
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if query {
                    vm.object_for_in_key_is_enumerable(&program, source, key)
                        .map(Vm::<TestHost>::integrity_bool)
                } else {
                    vm.object_for_in_keys(&program, source)
                };
                let succeeds = matches!(phase, "success" | "cycle" | "shadow" | "missing");
                assert_eq!(result.is_ok(), succeeds, "query={query} {phase}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) if query => {
                        assert_eq!(
                            result,
                            Vm::<TestHost>::integrity_bool(phase == "success")
                        );
                        None
                    }
                    Ok(result) => {
                        let elements = vm
                            .heap
                            .array_elements(result)
                            .map(|elements| elements.to_vec())
                            .expect("key snapshot");
                        let names = elements
                            .into_iter()
                            .map(|key| vm.to_string(&program, key).unwrap())
                            .collect::<Vec<_>>();
                        let expected = if phase == "cycle" {
                            vec!["first", "later"]
                        } else {
                            vec!["first", "later", "inherited"]
                        };
                        assert_eq!(names, expected);
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        None
                    }
                };
                if let Some(root) = key_root {
                    vm.heap.release_root(root);
                }
                vm.collect_now(&program);
                assert!(vm.heap.weak_value(source_handle).is_none());
                assert!(vm.heap.weak_value(key_handle).is_none());
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn proxy_introspection_roots_release_after_nested_validation() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for operation in ["prototype", "descriptor", "extensible"] {
            let phases: &[&str] = match operation {
                "prototype" => &[
                    "success",
                    "getter",
                    "trap",
                    "extensible",
                    "target-prototype",
                    "fallback",
                    "revoke-fallback",
                    "invalid-trap",
                    "invalid-result",
                    "mismatch",
                    "revoked",
                    "sealed",
                ],
                "descriptor" => &[
                    "success",
                    "key",
                    "getter",
                    "trap",
                    "target-descriptor",
                    "extensible",
                    "descriptor-value",
                    "fallback",
                    "revoke-fallback",
                    "invalid-trap",
                    "invalid-result",
                    "mismatch",
                    "revoked",
                    "hide-absent",
                    "hide-present",
                    "hide-frozen",
                ],
                _ => &[
                    "success",
                    "getter",
                    "trap",
                    "extensible",
                    "fallback",
                    "revoke-fallback",
                    "invalid-trap",
                    "mismatch",
                    "revoked",
                ],
            };
            for phase in phases {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function operands() {{
                        var raw=Object.create({{rank:46}});raw.entry={{rank:42}};
                        if ('{phase}'==='hide-frozen') Object.defineProperty(raw,'entry',{{configurable:false}});
                        if ('{operation}'==='prototype' && ('{phase}'==='target-prototype' || '{phase}'==='mismatch' || '{phase}'==='sealed')) Object.preventExtensions(raw);
                        var target=new Proxy(raw,{{
                            getOwnPropertyDescriptor(object,key) {{$262.gc();if ('{phase}'==='target-descriptor') throw {{kind:'target-descriptor'}};return Reflect.getOwnPropertyDescriptor(object,key);}},
                            get isExtensible() {{$262.gc();return function(object) {{$262.gc();if ('{phase}'==='extensible') throw {{kind:'extensible'}};return Reflect.isExtensible(object);}};}},
                            getPrototypeOf(object) {{$262.gc();if ('{phase}'==='target-prototype') throw {{kind:'target-prototype'}};return Reflect.getPrototypeOf(object);}}
                        }});
                        function method() {{$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                            if ('{phase}'==='invalid-trap') return 1;
                            if ('{phase}'==='revoke-fallback') {{revocable.revoke();$262.gc();return null;}}
                            if ('{phase}'==='fallback') return undefined;
                            return function() {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};
                                if ('{phase}'==='invalid-result') return 1;
                                if ('{operation}'==='extensible') return '{phase}'!=='mismatch';
                                if ('{operation}'==='prototype') return '{phase}'==='sealed' ? Reflect.getPrototypeOf(target) : {{rank:44}};
                                if ('{phase}'.startsWith('hide-')) return undefined;
                                return {{get value() {{$262.gc();if ('{phase}'==='descriptor-value') throw {{kind:'descriptor-value'}};return {{rank:43}};}},
                                    writable:true,enumerable:true,configurable:'{phase}'!=='mismatch'}};
                            }};
                        }}
                        var revocable=Proxy.revocable(target,{{get getPrototypeOf() {{return method();}},get getOwnPropertyDescriptor() {{return method();}},get isExtensible() {{return method();}}}});
                        if ('{phase}'==='revoked') revocable.revoke();
                        return [revocable.proxy,{{toString() {{$262.gc();if ('{phase}'==='key') throw {{kind:'key'}};return '{phase}'==='hide-absent' ? 'missing' : 'entry';}}}}];
                    }}
                "#
                );
                let program = compile(&source, "proxy-introspection-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let args = vm
                    .heap
                    .array_elements(operands)
                    .map(|elements| elements.to_vec())
                    .expect("operands");
                let handles = args
                    .iter()
                    .map(|value| vm.heap.weak_handle(*value).unwrap())
                    .collect::<Vec<_>>();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = match operation {
                    "prototype" => vm.object_get_prototype_of(&program, args[0]),
                    "descriptor" => vm.object_get_own_property_descriptor(&program, &args),
                    _ => vm.object_is_extensible(&program, &args[..1]),
                };
                let succeeds = matches!(
                    *phase,
                    "success"
                        | "fallback"
                        | "revoke-fallback"
                        | "sealed"
                        | "hide-absent"
                        | "hide-present"
                );
                assert_eq!(result.is_ok(), succeeds, "{operation} {phase}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) if operation == "extensible" => {
                        assert_eq!(result, Value::TRUE);
                        None
                    }
                    Ok(result) if phase.starts_with("hide-") => {
                        assert!(result.is_undefined());
                        None
                    }
                    Ok(result) => {
                        let rank_atom = vm.intern_atom("rank");
                        let value_atom = vm.intern_atom("value");
                        let value = if operation == "descriptor" {
                            vm.own_property(result, value_atom).unwrap()
                        } else {
                            result
                        };
                        let expected = if operation == "descriptor" {
                            if phase.ends_with("fallback") {
                                42.0
                            } else {
                                43.0
                            }
                        } else if phase.ends_with("fallback") || *phase == "sealed" {
                            46.0
                        } else {
                            44.0
                        };
                        assert_eq!(
                            vm.own_property(value, rank_atom),
                            Some(Value::number(expected))
                        );
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Err(error) => {
                        if matches!(
                            *phase,
                            "invalid-trap"
                                | "invalid-result"
                                | "mismatch"
                                | "revoked"
                                | "hide-frozen"
                        ) {
                            assert!(vm.format_error(&program, &error).contains("TypeError"));
                        } else {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                        None
                    }
                };
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none(), "{operation} {phase}");
                }
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn proxy_reads_root_operands_and_results_through_nested_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for symbol in [false, true] {
            for phase in [
                "success",
                "getter",
                "trap",
                "descriptor",
                "field",
                "fallback",
                "invalid-trap",
                "revoked",
                "frozen-same",
                "frozen-different",
                "no-getter-undefined",
                "no-getter-value",
                "result-symbol",
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function operands() {{
                        var target=new Proxy({{}},{{getOwnPropertyDescriptor(object,key) {{$262.gc();
                            if ('{phase}'==='descriptor') throw {{kind:'descriptor'}};
                            if ('{phase}'==='field') return {{get value() {{$262.gc();throw {{kind:'field'}};}},writable:true,enumerable:true,configurable:true}};
                            return Reflect.getOwnPropertyDescriptor(object,key);
                        }}}});
                        var handler={{get get() {{$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                            if ('{phase}'==='invalid-trap') return 1;
                            if ('{phase}'==='fallback') return null;
                            return function(object,key,receiver) {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};
                                if ('{phase}'==='result-symbol') return Symbol('answer');
                                if ('{phase}'==='frozen-same') return 42;
                                if ('{phase}'==='frozen-different') return 43;
                                if ('{phase}'==='no-getter-undefined') return undefined;
                                if ('{phase}'==='no-getter-value') return 42;
                                return {{rank:47,key:key,receiver:receiver}};
                            }};
                        }}}};
                        return [target,handler,{{rank:46}}];
                    }}
                "#
                );
                let program = compile(&source, "proxy-read-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let args = vm
                    .heap
                    .array_elements(operands)
                    .map(|elements| elements.to_vec())
                    .expect("operands");
                let handles = args
                    .iter()
                    .map(|value| vm.heap.weak_handle(*value).unwrap())
                    .collect::<Vec<_>>();
                let atom = vm.intern_atom("entry");
                let property = if symbol {
                    super::property_key::PropertyKey::symbol(
                        vm.heap.alloc(super::Cell::Symbol(crate::heap::SymbolData::public(Some("entry".into())))),
                    )
                } else {
                    super::property_key::PropertyKey::string(atom)
                };
                let key = if symbol {
                    property.symbol_value().unwrap()
                } else {
                    vm.heap.alloc_string("entry".into())
                };
                let key_handle = vm.heap.weak_handle(key).unwrap();
                if phase.starts_with("frozen-")
                    || phase.starts_with("no-getter-")
                    || phase == "fallback"
                {
                    let record = if phase.starts_with("no-getter-") {
                        super::object_descriptors::PropertyDescriptorRecord {
                            value: None,
                            writable: None,
                            getter: Some(Value::UNDEFINED),
                            setter: Some(Value::UNDEFINED),
                            enumerable: Some(true),
                            configurable: Some(false),
                        }
                    } else {
                        super::object_descriptors::PropertyDescriptorRecord {
                            writable: Some(false),
                            configurable: Some(false),
                            ..super::object_descriptors::PropertyDescriptorRecord::data(
                                Value::number(42.0),
                            )
                        }
                    };
                    vm.define_property_or_throw(&program, args[0], key, record)
                        .unwrap();
                }
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let handler = if phase == "revoked" {
                    Value::NULL
                } else {
                    args[1]
                };
                let result = vm.proxy_get(&program, args[0], handler, args[2], property);
                assert_eq!(
                    result.is_ok(),
                    matches!(
                        phase,
                        "success"
                            | "fallback"
                            | "frozen-same"
                            | "no-getter-undefined"
                            | "result-symbol"
                    ),
                    "symbol={symbol} {phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) if phase == "success" => {
                        let rank = vm.intern_atom("rank");
                        let receiver = vm.intern_atom("receiver");
                        let key_atom = vm.intern_atom("key");
                        assert_eq!(vm.own_property(result, rank), Some(Value::number(47.0)));
                        assert_eq!(vm.own_property(result, receiver), Some(args[2]));
                        assert_eq!(vm.own_property(args[2], rank), Some(Value::number(46.0)));
                        let returned_key = vm.own_property(result, key_atom).unwrap();
                        if symbol {
                            assert_eq!(returned_key, key);
                            assert!(
                                matches!(vm.heap.get(returned_key), Some(super::Cell::Symbol(crate::heap::SymbolData { description: Some(name), .. })) if name.as_ref() == "entry")
                            );
                        } else {
                            assert_eq!(vm.to_string(&program, returned_key).unwrap(), "entry");
                        }
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Ok(result) if phase == "result-symbol" => {
                        assert!(
                            matches!(vm.heap.get(result),Some(super::Cell::Symbol(crate::heap::SymbolData { description: Some(name), .. })) if name.as_ref()=="answer")
                        );
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Ok(result) => {
                        assert_eq!(
                            result,
                            if phase == "no-getter-undefined" {
                                Value::UNDEFINED
                            } else {
                                Value::number(42.0)
                            }
                        );
                        None
                    }
                    Err(error) => {
                        if matches!(
                            phase,
                            "invalid-trap" | "revoked" | "frozen-different" | "no-getter-value"
                        ) {
                            assert!(vm.format_error(&program, &error).contains("TypeError"));
                        } else {
                            let atom = vm.intern_atom("kind");
                            let kind = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        }
                        None
                    }
                };
                vm.collect_now(&program);
                for handle in handles {
                    assert!(
                        vm.heap.weak_value(handle).is_none(),
                        "symbol={symbol} {phase}"
                    );
                }
                // A raw handle names a record id, which a string allocated after the collection may
                // reuse: the key is gone when its record no longer holds it.
                assert!(
                    vm.heap.weak_value(key_handle).is_none()
                        || vm.heap.string(key).is_some_and(|text| text.host_string() != "entry"),
                    "symbol={symbol} {phase}"
                );
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn proxy_mutations_root_fresh_operands_and_restore_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for reflect in [false, true] {
            for prototype in [false, true] {
                for phase in [
                    "success",
                    "getter",
                    "trap",
                    "extensible",
                    "prototype",
                    "reject",
                    "invalid-trap",
                    "fallback",
                    "invariant",
                    "revoked",
                ] {
                    if !prototype && phase == "prototype" {
                        continue;
                    }
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                        function operand() {{
                            var raw={{}};
                            if ({prototype} && ('{phase}'==='prototype' || '{phase}'==='invariant')) Object.preventExtensions(raw);
                            var target=new Proxy(raw,{{
                                isExtensible(object) {{$262.gc();if ('{phase}'==='extensible') throw {{kind:'extensible'}};return Reflect.isExtensible(object);}},
                                getPrototypeOf(object) {{$262.gc();if ('{phase}'==='prototype') throw {{kind:'prototype'}};return Reflect.getPrototypeOf(object);}},
                                setPrototypeOf(object,proto) {{$262.gc();return Reflect.setPrototypeOf(object,proto);}},
                                preventExtensions(object) {{$262.gc();return Reflect.preventExtensions(object);}}
                            }});
                            function method() {{$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                                if ('{phase}'==='invalid-trap') return 1;
                                if ('{phase}'==='fallback') return null;
                                return function(object,proto) {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};
                                    if ('{phase}'==='reject') return false;
                                    if ('{phase}'==='invariant' || '{phase}'==='prototype' || '{phase}'==='extensible') return true;
                                    return {prototype} ? Reflect.setPrototypeOf(object,proto) : Reflect.preventExtensions(object);
                                }};
                            }}
                            if ('{phase}'==='revoked') {{var revocable=Proxy.revocable(target,{{}});revocable.revoke();return revocable.proxy;}}
                            return new Proxy(target,{{get setPrototypeOf() {{return method();}},get preventExtensions() {{return method();}}}});
                        }}
                    "#
                    );
                    let program = compile(&source, "proxy-mutation-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operand");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let target = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let target_handle = vm.heap.weak_handle(target).unwrap();
                    let proto = vm.object();
                    vm.set_named(&program, proto, "rank", Value::number(42.0))
                        .unwrap();
                    let proto_handle = vm.heap.weak_handle(proto).unwrap();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let native = match (reflect, prototype) {
                        (false, false) => Native::ObjectPreventExtensions,
                        (false, true) => Native::ObjectSetPrototypeOf,
                        (true, false) => Native::ReflectPreventExtensions,
                        (true, true) => Native::ReflectSetPrototypeOf,
                    };
                    let args = if prototype {
                        vec![target, proto]
                    } else {
                        vec![target]
                    };
                    let result = if reflect {
                        vm.call_reflect_native(&program, native, &args)
                    } else {
                        vm.call_object_native(&program, native, &args)
                    };
                    let success = phase == "success" || phase == "fallback";
                    assert_eq!(
                        result.is_ok(),
                        success || (reflect && phase == "reject"),
                        "reflect={reflect} prototype={prototype} {phase}"
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(result) => {
                            assert_eq!(
                                result,
                                if reflect {
                                    Vm::<TestHost>::integrity_bool(success)
                                } else {
                                    target
                                }
                            );
                            assert_eq!(
                                vm.heap.weak_value(target_handle),
                                Some(target),
                                "reflect={reflect} prototype={prototype} {phase}"
                            );
                            if prototype {
                                assert_eq!(vm.heap.weak_value(proto_handle), Some(proto));
                                let rank = vm.intern_atom("rank");
                                assert_eq!(vm.own_property(proto, rank), Some(Value::number(42.0)));
                                if success {
                                    let raw = vm.proxy_target(target);
                                    assert_eq!(vm.object_data(raw).unwrap().proto(), proto);
                                }
                            } else if success {
                                let raw = vm.proxy_target(target);
                                assert!(!vm.object_data(raw).unwrap().is_extensible());
                            }
                        }
                        Err(error) => {
                            if matches!(phase, "reject" | "invalid-trap" | "invariant" | "revoked")
                            {
                                assert!(vm.format_error(&program, &error).contains("TypeError"));
                            } else {
                                let atom = vm.intern_atom("kind");
                                let kind = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                            }
                        }
                    }
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(target_handle).is_none());
                    assert!(vm.heap.weak_value(proto_handle).is_none());
                }
            }
        }
    }
}

#[test]
fn proxy_invocations_root_fresh_operands_and_restore_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for construct in [false, true] {
            for phase in [
                "success",
                "fallback",
                "getter",
                "trap",
                "invalid-trap",
                "primitive-result",
                "revoked",
            ] {
                if !construct && phase == "primitive-result" {
                    continue;
                }
                let mut vm = Vm::new(TestHost::default().with_test262());
                let method = if construct { "construct" } else { "apply" };
                let source = format!(
                    r#"
                    function operand() {{
                        function target(arg) {{ $262.gc(); return {{rank:42,arg:arg,receiver:this}}; }}
                        var handler={{get {method}() {{
                            $262.gc();
                            if ('{phase}'==='getter') throw {{kind:'getter'}};
                            if ('{phase}'==='fallback') return null;
                            if ('{phase}'==='invalid-trap') return 1;
                            return function(target,second,third) {{
                                $262.gc();
                                if ('{phase}'==='trap') throw {{kind:'trap'}};
                                if ('{phase}'==='primitive-result') return 1;
                                return {{rank:42,arg:{construct} ? second[0] : third[0],receiver:{construct} ? third : second}};
                            }};
                        }}}};
                        if ('{phase}'==='revoked') {{var r=Proxy.revocable(target,handler);r.revoke();return r.proxy;}}
                        return new Proxy(target,handler);
                    }}
                "#
                );
                let program = compile(&source, "proxy-invocation-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operand");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let proxy = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let receiver = vm.object();
                vm.set_named(&program, receiver, "rank", Value::number(46.0))
                    .unwrap();
                let arg = vm.object();
                vm.set_named(&program, arg, "rank", Value::number(47.0))
                    .unwrap();
                let handles =
                    [proxy, receiver, arg].map(|value| vm.heap.weak_handle(value).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if construct {
                    vm.proxy_construct(&program, proxy, proxy, &[arg])
                } else {
                    vm.proxy_call(&program, proxy, receiver, &[arg])
                };
                assert_eq!(
                    result.is_ok(),
                    matches!(phase, "success" | "fallback"),
                    "construct={construct} {phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) => {
                        assert_eq!(
                            vm.heap.weak_value(handles[0]),
                            Some(proxy),
                            "construct={construct} {phase}"
                        );
                        assert_eq!(vm.heap.weak_value(handles[2]), Some(arg));
                        let rank = vm.intern_atom("rank");
                        assert_eq!(vm.own_property(result, rank), Some(Value::number(42.0)));
                        assert_eq!(vm.own_property(arg, rank), Some(Value::number(47.0)));
                        let atom = vm.intern_atom("arg");
                        assert_eq!(vm.own_property(result, atom), Some(arg));
                        if !construct || phase == "success" {
                            let atom = vm.intern_atom("receiver");
                            assert_eq!(
                                vm.own_property(result, atom),
                                Some(if construct { proxy } else { receiver })
                            );
                            if !construct {
                                assert_eq!(
                                    vm.own_property(receiver, rank),
                                    Some(Value::number(46.0))
                                );
                            }
                        }
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Err(error) => {
                        if matches!(phase, "getter" | "trap") {
                            let atom = vm.intern_atom("kind");
                            let kind = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        } else {
                            assert!(
                                vm.format_error(&program, &error).contains("TypeError"),
                                "construct={construct} {phase}: {}",
                                vm.format_error(&program, &error)
                            );
                        }
                        None
                    }
                };
                vm.collect_now(&program);
                for handle in handles {
                    assert!(
                        vm.heap.weak_value(handle).is_none(),
                        "construct={construct} {phase}"
                    );
                }
                if let Some(handle) = result_handle {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn proxy_presence_and_delete_root_fresh_operands_and_restore_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for remove in [false, true] {
            for symbol in [false, true] {
                for phase in [
                    "success",
                    "skip-validation",
                    "getter",
                    "trap",
                    "descriptor",
                    "field",
                    "extensible",
                    "invalid-trap",
                    "fallback",
                    "nonconfig",
                    "nonext",
                    "revoked",
                    "coercion",
                ] {
                    if symbol && phase == "coercion" {
                        continue;
                    }
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let method = if remove { "deleteProperty" } else { "has" };
                    let source = format!(
                        r#"
                        function operands() {{
                            var raw={{entry:42}};
                            if ('{phase}'==='nonconfig') Object.defineProperty(raw,'entry',{{configurable:false}});
                            if ('{phase}'==='nonext') Object.preventExtensions(raw);
                            var target=new Proxy(raw,{{
                                getOwnPropertyDescriptor(object,key) {{$262.gc();
                                    if ('{phase}'==='descriptor') throw {{kind:'descriptor'}};
                                    if ('{phase}'==='field') return {{value:42,writable:true,enumerable:true,get configurable() {{$262.gc();throw {{kind:'field'}};}}}};
                                    return Reflect.getOwnPropertyDescriptor(object,key);
                                }},
                                isExtensible(object) {{$262.gc();if ('{phase}'==='extensible') throw {{kind:'extensible'}};return Reflect.isExtensible(object);}}
                            }});
                            var handler={{get {method}() {{$262.gc();
                                if ('{phase}'==='getter') throw {{kind:'getter'}};
                                if ('{phase}'==='invalid-trap') return 1;
                                if ('{phase}'==='fallback') return null;
                                return function(object,key) {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};
                                    return '{phase}'==='skip-validation' ? !{remove} : {remove};
                                }};
                            }}}};
                            if ('{phase}'==='revoked') {{var r=Proxy.revocable(target,handler);r.revoke();return [r.proxy,raw];}}
                            return [new Proxy(target,handler),raw];
                        }}
                        function keyOperand() {{return {{toString() {{$262.gc();throw {{kind:'coercion'}};}}}};}}
                    "#
                    );
                    let program = compile(&source, "proxy-presence-delete-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operands");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let values = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let (proxy, raw) = vm
                        .heap
                        .array_elements(values)
                        .map(|elements| (elements[0], elements[1]))
                        .expect("operands");
                    let key = if symbol {
                        vm.heap.alloc(super::Cell::Symbol(crate::heap::SymbolData::public(Some("entry".into()))))
                    } else if phase == "coercion" {
                        let atom = vm.intern_atom("keyOperand");
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        vm.call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap()
                    } else {
                        vm.heap.alloc_string("entry".into())
                    };
                    if symbol {
                        if phase == "nonext" {
                            vm.object_data_mut(raw).unwrap().set_extensible(true);
                        }
                        vm.define_property_or_throw(
                            &program,
                            raw,
                            key,
                            super::object_descriptors::PropertyDescriptorRecord {
                                configurable: Some(phase != "nonconfig"),
                                ..super::object_descriptors::PropertyDescriptorRecord::data(
                                    Value::number(42.0),
                                )
                            },
                        )
                        .unwrap();
                        if phase == "nonext" {
                            vm.object_data_mut(raw).unwrap().set_extensible(false);
                        }
                    }
                    let handles = [proxy, key].map(|value| vm.heap.weak_handle(value).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = vm.call_reflect_native(
                        &program,
                        if remove {
                            Native::ReflectDeleteProperty
                        } else {
                            Native::ReflectHas
                        },
                        &[proxy, key],
                    );
                    assert_eq!(
                        result.is_ok(),
                        matches!(phase, "success" | "skip-validation" | "fallback"),
                        "remove={remove} symbol={symbol} {phase}"
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(result) => {
                            assert_eq!(
                                result,
                                Vm::<TestHost>::integrity_bool(if phase == "fallback" {
                                    true
                                } else if phase == "skip-validation" {
                                    !remove
                                } else {
                                    remove
                                })
                            );
                            assert_eq!(
                                vm.heap.weak_value(handles[0]),
                                Some(proxy),
                                "remove={remove} symbol={symbol} {phase}"
                            );
                            assert_eq!(vm.heap.weak_value(handles[1]), Some(key));
                            assert!(if symbol {
                                matches!(vm.heap.get(key),Some(super::Cell::Symbol(crate::heap::SymbolData { description: Some(name), .. })) if name.as_ref()=="entry")
                            } else {
                                vm.heap.string(key).is_some_and(|name| name.host_string()=="entry")
                            });
                        }
                        Err(error) => {
                            if matches!(phase, "invalid-trap" | "nonconfig" | "nonext" | "revoked")
                            {
                                assert!(vm.format_error(&program, &error).contains("TypeError"));
                            } else {
                                let atom = vm.intern_atom("kind");
                                let kind = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                            }
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(
                            vm.heap.weak_value(handle).is_none(),
                            "remove={remove} symbol={symbol} {phase}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn proxy_revocation_consumes_captured_ownership_and_preserves_kind() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, body, callable, constructable) in [
            ("object", "return {};", false, false),
            ("callable", "return ()=>42;", true, false),
            ("constructor", "return function(){return 42;};", true, true),
        ] {
            for owner in ["proxy", "revoker", "record"] {
                for entry in ["direct", "call", "guarded"] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let program = compile(
                        &format!("function operand() {{{body}}}"),
                        "proxy-revocation-owners.js",
                    )
                    .unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operand");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let target = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let handler = vm.object();
                    let record = vm.proxy_revocable(&program, &[target, handler]).unwrap();
                    let atom = vm.intern_atom("proxy");
                    let proxy = vm.own_property(record, atom).unwrap();
                    let atom = vm.intern_atom("revoke");
                    let revoke = vm.own_property(record, atom).unwrap();
                    let handles = [target, handler, proxy, revoke, record]
                        .map(|value| vm.heap.weak_handle(value).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let active_native = vm.realm.promise.active_native.len();
                    let realm = vm.realm.globals;
                    let revoke_root = vm.heap.root(revoke);
                    let owner_root = match owner {
                        "proxy" => Some(vm.heap.root(proxy)),
                        "record" => Some(vm.heap.root(record)),
                        _ => None,
                    };
                    if owner == "record" {
                        let key = vm.heap.alloc_string("proxy".into());
                        assert_eq!(
                            vm.object_delete_property(&program, &[record, key]).unwrap(),
                            Value::TRUE
                        );
                    }
                    vm.collect_now(&program);
                    for (handle, value) in handles[..4].iter().zip([target, handler, proxy, revoke])
                    {
                        assert_eq!(
                            vm.heap.weak_value(*handle),
                            Some(value),
                            "{kind} {owner} {entry}"
                        );
                    }
                    for _ in 0..2 {
                        let result = match entry {
                            "direct" => vm.proxy_revoke(revoke),
                            "call" => vm.call_value(&program, revoke, Value::TRUE, &[Value::FALSE]),
                            _ => vm.call_native_guarded(
                                &program,
                                Native::ProxyRevoke,
                                Value::TRUE,
                                &[Value::FALSE],
                                revoke,
                            ),
                        };
                        assert_eq!(result.unwrap(), Value::UNDEFINED);
                        assert_eq!(vm.active_call_roots.len(), calls);
                        assert_eq!(vm.realm.promise.active_native.len(), active_native);
                        assert_eq!(vm.realm.globals, realm);
                    }
                    assert_eq!(vm.is_function(proxy), callable, "{kind} {owner} {entry}");
                    assert_eq!(
                        vm.is_constructable(&program, proxy),
                        constructable,
                        "{kind} {owner} {entry}"
                    );
                    if owner != "revoker" {
                        vm.heap.release_root(revoke_root);
                    }
                    vm.collect_now(&program);
                    assert!(
                        vm.heap.weak_value(handles[0]).is_none(),
                        "target retained: {kind} {owner} {entry}"
                    );
                    assert!(
                        vm.heap.weak_value(handles[1]).is_none(),
                        "handler retained: {kind} {owner} {entry}"
                    );
                    assert_eq!(
                        vm.heap.weak_value(handles[2]),
                        (owner == "proxy").then_some(proxy),
                        "{kind} {owner} {entry}"
                    );
                    assert_eq!(
                        vm.heap.weak_value(handles[3]),
                        (owner != "proxy").then_some(revoke)
                    );
                    assert_eq!(
                        vm.heap.weak_value(handles[4]),
                        (owner == "record").then_some(record)
                    );
                    if owner != "proxy" {
                        assert!(
                            matches!(vm.heap.function(revoke),Some(crate::heap::FunctionRecord {env,..}) if env.is_null())
                        );
                        assert_eq!(
                            vm.call_value(&program, revoke, Value::UNDEFINED, &[])
                                .unwrap(),
                            Value::UNDEFINED
                        );
                    }
                    if let Some(root) = owner_root {
                        vm.heap.release_root(root);
                    } else {
                        vm.heap.release_root(revoke_root);
                    }
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(
                            vm.heap.weak_value(handle).is_none(),
                            "{kind} {owner} {entry}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn proxy_own_keys_keeps_captured_target_after_revoking_getter() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for trapped in [false, true] {
            for phase in ["success", "getter", "target", "trap"] {
                if !trapped && phase == "trap" {
                    continue;
                }
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function operand() {{
                        var revoke;
                        var r=Proxy.revocable(new Proxy({{first:42,[Symbol('entry')]:43}},{{ownKeys(object) {{
                            $262.gc();if ('{phase}'==='target') throw {{kind:'target'}};return Reflect.ownKeys(object);
                        }}}}),{{get ownKeys() {{
                            revoke();$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                            if (!{trapped}) return null;
                            return function(object) {{$262.gc();if ('{phase}'==='trap') throw {{kind:'trap'}};return Reflect.ownKeys(object);}};
                        }}}});
                        revoke=r.revoke;
                        return r.proxy;
                    }}
                "#
                );
                let program = compile(&source, "proxy-own-keys-revocation-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operand");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let proxy = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let (target, handler) = match vm.heap.get(proxy) {
                    Some(super::Cell::Proxy {
                        target, handler, ..
                    }) => (*target, *handler),
                    _ => panic!("proxy"),
                };
                let handles =
                    [proxy, target, handler].map(|value| vm.heap.weak_handle(value).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.proxy_own_keys(&program, proxy);
                assert_eq!(
                    result.is_ok(),
                    phase == "success",
                    "trapped={trapped} {phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let keys = match result {
                    Ok(keys) => {
                        assert_eq!(keys.len(), 2);
                        assert_eq!(vm.to_string(&program, keys[0]).unwrap(), "first");
                        assert!(
                            matches!(vm.heap.get(keys[1]),Some(super::Cell::Symbol(crate::heap::SymbolData { description: Some(name), .. })) if name.as_ref()=="entry")
                        );
                        for (handle, value) in handles.iter().zip([proxy, target, handler]) {
                            assert_eq!(
                                vm.heap.weak_value(*handle),
                                Some(value),
                                "trapped={trapped} {phase}"
                            );
                        }
                        assert!(
                            matches!(vm.heap.get(proxy),Some(super::Cell::Proxy {target,handler,..}) if target.is_null()&&handler.is_null())
                        );
                        // A key a program spells is its shared atom string (`Vm::atom_string`),
                        // kept with the program; the symbol key is the proxy target's own.
                        let first = vm.intern_atom("first");
                        assert_eq!(keys[0], vm.atom_string(first));
                        vec![vm.heap.weak_handle(keys[1]).unwrap()]
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        Vec::new()
                    }
                };
                vm.collect_now(&program);
                for handle in handles.into_iter().chain(keys) {
                    assert!(
                        vm.heap.weak_value(handle).is_none(),
                        "trapped={trapped} {phase}"
                    );
                }
            }
        }
    }
}

#[test]
fn coerced_binary_operands_restore_roots_after_each_completion() {
    use oxc_ast::ast::BinaryOperator;
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in ["string", "numeric-string", "bigint"] {
            for (operator, token) in [
                (BinaryOperator::LessThan, "<"),
                (BinaryOperator::GreaterEqualThan, ">="),
                (BinaryOperator::Addition, "+"),
                (BinaryOperator::Subtraction, "-"),
            ] {
                for phase in ["success", "left", "right"] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                        function operands() {{return [
                            {{get [Symbol.toPrimitive]() {{$262.gc();if ('{phase}'==='left') throw {{kind:'left'}};
                                return function(hint) {{$262.gc();return '{kind}'==='bigint'?BigInt(42):'{kind}'==='numeric-string'?String.fromCharCode(52,50):String.fromCharCode(97,98);}};
                            }}}},
                            {{get [Symbol.toPrimitive]() {{$262.gc();if ('{phase}'==='right') throw {{kind:'right'}};
                                return function(hint) {{$262.gc();return '{kind}'==='bigint'?BigInt(7):'{kind}'==='numeric-string'?String.fromCharCode(55):String.fromCharCode(99);}};
                            }}}}
                        ];}}
                    "#
                    );
                    let program = compile(&source, "binary-coercion-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operands");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let values = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let (left, right) = vm
                        .heap
                        .array_elements(values)
                        .map(|elements| (elements[0], elements[1]))
                        .expect("operands");
                    let handles = [left, right].map(|value| vm.heap.weak_handle(value).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = vm.binary(&program, operator as u32, left, right);
                    assert_eq!(result.is_ok(), phase == "success", "{kind} {token} {phase}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    let result_handle = match result {
                        Ok(value) => {
                            let expected = match token {
                                "<" => {
                                    if kind == "bigint" {
                                        "false"
                                    } else {
                                        "true"
                                    }
                                }
                                ">=" => {
                                    if kind == "bigint" {
                                        "true"
                                    } else {
                                        "false"
                                    }
                                }
                                "+" => match kind {
                                    "bigint" => "49",
                                    "numeric-string" => "427",
                                    _ => "abc",
                                },
                                _ => {
                                    if kind == "string" {
                                        "NaN"
                                    } else {
                                        "35"
                                    }
                                }
                            };
                            assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                expected,
                                "{kind} {token}"
                            );
                            for (handle, value) in handles.iter().zip([left, right]) {
                                assert_eq!(vm.heap.weak_value(*handle), Some(value));
                            }
                            value.is_heap().then(|| vm.heap.weak_handle(value).unwrap())
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let kind = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                            None
                        }
                    };
                    vm.collect_now(&program);
                    for handle in handles.into_iter().chain(result_handle) {
                        assert!(
                            vm.heap.weak_value(handle).is_none(),
                            "{kind} {token} {phase}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn numeric_binary_fast_paths_preserve_number_edges_and_generic_coercion() {
    let source = r#"
        function assert(condition) {
            if (!condition) throw new Error('numeric fast-path mismatch');
        }
        assert(Object.is(0 * -1, -0));
        assert(Object.is(-2 % 2, -0));
        assert(Object.is(0 / -1, -0));
        assert(Object.is((-2147483648) % -1, -0));
        assert(6 / 3 === 2);
        assert(1 / 2 === 0.5);
        assert((2147483647 + 1) === 2147483648);
        assert((1 << 31) === -2147483648);
        assert((1 << 33) === 2);
        assert((-1 >>> 0) === 4294967295);
        assert(!((0 / 0) < 1));

        let order = '';
        const left = { valueOf() { order += 'l'; return 9; } };
        const right = { valueOf() { order += 'r'; return 4; } };
        assert(left - right === 5 && order === 'lr');

        let mixedBigIntThrows = false;
        try { 1n - 1; } catch (error) { mixedBigIntThrows = true; }
        assert(mixedBigIntThrows);
    "#;
    let program = Engine::specialize(source, "numeric-binary-fast-paths.js").unwrap();
    Vm::new(TestHost::default()).execute(&program).unwrap();
}

#[test]
fn equality_roots_the_opposite_primitive_during_object_coercion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for bigint in [false, true] {
            for reversed in [false, true] {
                for throws in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"function operand() {{return {{[Symbol.toPrimitive]() {{$262.gc();if ({throws}) throw {{kind:'coercion'}};return {bigint}?BigInt(43):String.fromCharCode(97,99);}}}};}}"#
                    );
                    let program = compile(&source, "equality-coercion-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operand");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let object = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let primitive = if bigint {
                        vm.heap.alloc(super::Cell::bigint(42.into()))
                    } else {
                        vm.heap.alloc_string("ab".into())
                    };
                    let handles =
                        [object, primitive].map(|value| vm.heap.weak_handle(value).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = if reversed {
                        vm.equal(&program, object, primitive)
                    } else {
                        vm.equal(&program, primitive, object)
                    };
                    assert_eq!(
                        result.is_ok(),
                        !throws,
                        "bigint={bigint} reversed={reversed} throws={throws}"
                    );
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(value) => {
                            assert!(!value);
                            assert_eq!(
                                vm.to_string(&program, primitive).unwrap(),
                                if bigint { "42" } else { "ab" }
                            );
                            assert_eq!(vm.heap.weak_value(handles[1]), Some(primitive));
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let kind = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, kind).unwrap(), "coercion");
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn instanceof_roots_fresh_inputs_across_lookup_and_traversal() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for ordinary in [false, true] {
            for phase in ["success", "prototype", "traversal"] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function operands() {{
                        function C() {{}}
                        return [new Proxy(Object.create(C.prototype), {{getPrototypeOf(target) {{
                            $262.gc(); if ('{phase}'==='traversal') throw {{kind:'traversal'}};
                            return Reflect.getPrototypeOf(target);
                        }}}}), new Proxy(C, {{get(target,key) {{
                            $262.gc();
                            if (key===Symbol.hasInstance) return null;
                            if (key==='prototype' && '{phase}'==='prototype') throw {{kind:'prototype'}};
                            return Reflect.get(target,key);
                        }}}})];
                    }}
                "#
                );
                let program = compile(&source, "instanceof-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let values = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let (value, constructor) = vm
                    .heap
                    .array_elements(values)
                    .map(|elements| (elements[0], elements[1]))
                    .expect("operands");
                let handles = [value, constructor].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if ordinary {
                    vm.ordinary_has_instance(&program, constructor, value)
                } else {
                    vm.instanceof(&program, value, constructor)
                };
                assert_eq!(
                    result.is_ok(),
                    phase == "success",
                    "ordinary={ordinary} {phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                match result {
                    Ok(result) => {
                        assert!(result, "ordinary={ordinary}");
                        for (handle, value) in handles.iter().zip([value, constructor]) {
                            assert_eq!(vm.heap.weak_value(*handle), Some(value));
                        }
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn instanceof_roots_detached_prototype_and_restores_cursor_after_throw() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for ordinary in [false, true] {
            for throws in [false, true] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function operands() {{
                        function C() {{}}
                        Object.defineProperty(C,Symbol.hasInstance,{{value:null}});
                        return [new Proxy({{}},{{getPrototypeOf() {{
                            C.prototype={{}}; $262.gc();
                            return new Proxy({{}},{{getPrototypeOf() {{
                                $262.gc(); if ({throws}) throw {{kind:'cursor'}}; return null;
                            }}}});
                        }}}}),C,C.prototype];
                    }}
                "#
                );
                let program = compile(&source, "instanceof-detached-prototype.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("operands");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let operands = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let values = vm
                    .heap
                    .array_elements(operands)
                    .map(|elements| [elements[0], elements[1], elements[2]])
                    .expect("operands");
                let handles = values.map(|value| vm.heap.weak_handle(value).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = if ordinary {
                    vm.ordinary_has_instance(&program, values[1], values[0])
                } else {
                    vm.instanceof(&program, values[0], values[1])
                };
                assert_eq!(result.is_err(), throws);
                match result {
                    Ok(result) => assert!(!result),
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), "cursor");
                    }
                }
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip(values) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value));
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn iterator_step_roots_receiver_through_result_accessors() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for cached in [false, true] {
            for wrapped in [false, true] {
                for phase in ["value", "done", "done-throw", "value-throw", "invalid"] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                    function step() {{if ('{phase}'==='invalid') return 42; return {{
                            get done() {{$262.gc(); if ('{phase}'==='done-throw') throw {{kind:'done-throw'}}; return '{phase}'==='done';}},
                            get value() {{$262.gc(); if ('{phase}'==='value-throw') throw {{kind:'value-throw'}}; return {{tag:42}};}}
                        }};}}
                    function operand() {{var iterator={{next() {{$262.gc();return step();}}}};return {wrapped}?Iterator.from(iterator):iterator;}}
                "#
                    );
                    let program = compile(&source, "iterator-step-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("operand");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let iterator = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let handle = vm.heap.weak_handle(iterator).unwrap();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = if cached {
                        let iterator_root = vm.heap.root(iterator);
                        let atom = vm.intern_atom("next");
                        let method = vm.get_property(&program, iterator, atom).unwrap();
                        let method_root = vm.heap.root(method);
                        let result =
                            vm.rooted_iterator_step_value(&program, iterator_root, method_root);
                        vm.heap.release_root(method_root);
                        vm.heap.release_root(iterator_root);
                        result
                    } else {
                        vm.iterator_step_value(&program, iterator)
                    };
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    assert_eq!(
                        vm.heap.weak_value(handle),
                        Some(iterator),
                        "cached={cached} {phase}"
                    );
                    let value_handle = match result {
                        Ok(Some(value)) => {
                            assert_eq!(phase, "value");
                            let atom = vm.intern_atom("tag");
                            assert_eq!(
                                vm.own_property(value, atom).unwrap().as_number(),
                                Some(42.0)
                            );
                            Some(vm.heap.weak_handle(value).unwrap())
                        }
                        Ok(None) => {
                            assert_eq!(phase, "done");
                            None
                        }
                        Err(error) => {
                            if phase == "invalid" {
                                assert!(error.to_string().contains("not an object"));
                            } else {
                                assert!(phase.ends_with("throw"));
                                let atom = vm.intern_atom("kind");
                                let kind = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                            }
                            None
                        }
                    };
                    vm.collect_now(&program);
                    for handle in std::iter::once(handle).chain(value_handle) {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn suspended_owners_trace_complete_frame_and_request_state() {
    use super::activation::{AsyncGeneratorOperation, AsyncGeneratorRequest, GeneratorRecord};
    for completion in [
        Completion::Return,
        Completion::Throw,
        Completion::Yield,
        Completion::Await,
    ] {
        for generator in [false, true] {
            let mut vm = Vm::new(TestHost::default());
            let program = Engine::specialize("", "suspended-owner-roots.js").unwrap();
            vm.initialize(&program).unwrap();
            let env = vm.object();
            let receiver = vm.object();
            let local = vm.object();
            let binding = vm.object();
            let register = vm.object();
            let iterator = vm.object();
            let completion_value = vm.object();
            let promise = vm.object();
            let realm = vm.object();
            let request_promise = vm.object();
            let request_value = vm.object();
            let frame_values = [
                env,
                receiver,
                local,
                binding,
                register,
                iterator,
                completion_value,
                promise,
            ];
            let owned = if generator {
                frame_values
                    .into_iter()
                    .chain([realm, request_promise, request_value])
                    .collect::<Vec<_>>()
            } else {
                frame_values.to_vec()
            };
            let handles = owned
                .iter()
                .map(|v| vm.heap.weak_handle(*v).unwrap())
                .collect::<Vec<_>>();
            let atom = vm.intern_atom("binding");
            let frame = super::Frame {
                context: CallContext::Internal,
                reflected_parameters: 0,
                original_arguments: vec![],
                program: super::program_store::ProgramId::MAIN,
                function: 0,
                pc: 0,
                pc_is_resume: true,
                js_jit_skip_entry_once: false,
                source_field: None,
                binding_site_pc: None,
                env,
                this: receiver,
                locals: vec![local],
                dynamic_bindings: vec![(atom, binding)],
                captured: true,
                registers: vec![register, iterator, Value::FALSE],
                initialized_registers: 0,
                capture_cache: super::CaptureCache::default(),
                active_iterators: vec![super::ActiveIterator {
                    iterator: 1,
                    done: 2,
                }],
                with_objects: Vec::new(),
                with_base: 0,
                js_jit_interpreter_started: None,
                fixed_this: false,
            };
            let continuation = Continuation::suspend(
                Box::new(frame),
                completion(completion_value),
                Some(0),
                promise,
            );
            let roots = vm.heap.root_count_for_test();
            let owner = if generator {
                let value = vm.heap.alloc_iterator(crate::heap::IteratorRecord {
                    object: Vm::<TestHost>::empty_object(Value::NULL),
                    source: Value::NULL,
                    kind: super::IteratorKind::AsyncGenerator,
                    index: 0,
                    done: false,
                    ext: crate::heap::IteratorExt {
                        position_high: 0,
                        next_method: None,
                        helper: None,
                        helper_running: false,
                        helper_started: false,
                        generator: Some(Box::new(GeneratorRecord {
                            continuation: Some(continuation),
                            realm,
                            done: false,
                            running: false,
                            requests: [AsyncGeneratorRequest {
                                operation: AsyncGeneratorOperation::Next,
                                promise: request_promise,
                                value: request_value,
                            }]
                            .into(),
                        })),
                    },
                });
                vm.heap.root(value)
            } else {
                let value = vm.heap.alloc(super::Cell::AwaitState(Box::new(
                    crate::heap::AwaitState {
                        continuation: Some(Box::new(continuation)),
                        realm: Value::UNDEFINED,
                        promise,
                        generator: None,
                        yielded: false,
                    },
                )));
                vm.heap.root(value)
            };
            vm.collect_now(&program);
            for (handle, value) in handles.iter().zip(&owned) {
                assert_eq!(
                    vm.heap.weak_value(*handle),
                    Some(*value),
                    "generator={generator} completion={:?}",
                    completion(completion_value)
                );
            }
            let value = vm.heap.root_value(owner).unwrap();
            let resumed = if let Some(state) = vm.heap.await_state_mut(value) {
                *state.continuation.take().unwrap()
            } else {
                match vm.heap.iterator_mut(value) {
                Some(crate::heap::IteratorRecord { ext, .. }) => ext
                    .generator
                    .as_mut()
                    .expect("generator owner")
                    .continuation
                    .take()
                    .unwrap(),
                _ => panic!("suspended owner"),
                }
            };
            assert_eq!(resumed.completion, completion(completion_value));
            assert_eq!(resumed.promise, promise);
            let resumed = resumed.resume(0);
            assert_eq!(resumed.locals, [local]);
            assert_eq!(resumed.dynamic_bindings, [(atom, binding)]);
            assert_eq!(resumed.registers, [register, iterator, Value::FALSE]);
            assert_eq!(
                resumed.active_iterators,
                [super::ActiveIterator {
                    iterator: 1,
                    done: 2
                }]
            );
            vm.heap.release_root(owner);
            assert_eq!(vm.heap.root_count_for_test(), roots);
            vm.collect_now(&program);
            for handle in handles {
                assert!(vm.heap.weak_value(handle).is_none());
            }
        }
    }
}

#[test]
fn array_iterator_advance_roots_owner_and_applies_index_transition() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [Native::ArrayKeys, Native::ArrayValues, Native::ArrayEntries] {
            for phase in ["value", "length", "item", "done"] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"function source() {{return {{
                    get length() {{$262.gc(); if ('{phase}'==='length') throw {{kind:'length'}}; return '{phase}'==='done'?0:2;}},
                    get 0() {{$262.gc(); if ('{phase}'==='item') throw {{kind:'item'}};return {{tag:42}};}}
                }};}}"#
                );
                let program = compile(&source, "array-iterator-advance.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("source");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let source = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let iterator = vm.array_iterator_native(&program, native, source).unwrap();
                let handles = [iterator, source].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.iterator_next(&program, iterator);
                let throws = phase == "length" || (phase == "item" && native != Native::ArrayKeys);
                assert_eq!(result.is_err(), throws);
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                assert_eq!(
                    vm.heap.weak_value(handles[0]),
                    Some(iterator),
                    "{native:?} {phase}"
                );
                match result {
                    Ok(result) => {
                        let atom = vm.intern_atom("done");
                        assert_eq!(
                            vm.own_property(result, atom),
                            Some(if phase == "done" {
                                Value::TRUE
                            } else {
                                Value::FALSE
                            })
                        );
                        if phase != "done" {
                            let atom = vm.intern_atom("value");
                            let mut value = vm.own_property(result, atom).unwrap();
                            if native == Native::ArrayEntries {
                                value = vm
                                    .heap
                                    .array_elements(value)
                                    .map(|elements| {
                                        assert_eq!(elements[0].as_number(), Some(0.0));
                                        elements[1]
                                    })
                                    .expect("entry");
                            }
                            if native == Native::ArrayKeys {
                                assert_eq!(value.as_number(), Some(0.0));
                            } else {
                                let atom = vm.intern_atom("tag");
                                assert_eq!(
                                    vm.own_property(value, atom).unwrap().as_number(),
                                    Some(42.0)
                                );
                            }
                        }
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    }
                }
                match vm.heap.iterator(iterator) {
                    Some(crate::heap::IteratorRecord {
                        index,
                        done,
                        source: owner,
                        ..
                    }) => {
                        assert_eq!(
                            *index,
                            if phase == "length" || phase == "done" {
                                0
                            } else {
                                1
                            }
                        );
                        assert_eq!(*done, phase == "done");
                        assert_eq!(
                            *owner,
                            if phase == "done" {
                                Value::UNDEFINED
                            } else {
                                source
                            }
                        );
                    }
                    _ => panic!("iterator"),
                }
                let owner = vm.heap.root(iterator);
                vm.collect_now(&program);
                assert_eq!(vm.heap.weak_value(handles[1]).is_none(), phase == "done");
                vm.heap.release_root(owner);
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn array_iterator_retains_captured_source_after_reentrant_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let mut vm = Vm::new(TestHost::default().with_test262());
        let program = compile(
            r#"
            var iterator,nested=false;
            function lengthValue() {return {[Symbol.toPrimitive]() {
                nested=true;iterator.next();$262.gc();return 1;
            }};}
            function source() {return {
                get length() {return nested?0:lengthValue();},
                get 0() {$262.gc();return {tag:42};}
            };}
        "#,
            "array-iterator-captured-source.js",
        )
        .unwrap();
        vm.execute(&program).unwrap();
        let atom = vm.intern_atom("source");
        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
        let source = vm
            .call_value(&program, factory, Value::UNDEFINED, &[])
            .unwrap();
        let iterator = vm
            .array_iterator_native(&program, Native::ArrayValues, source)
            .unwrap();
        let atom = vm.intern_atom("iterator");
        vm.set_property(vm.realm.globals, atom, iterator).unwrap();
        let handles = [source, iterator].map(|v| vm.heap.weak_handle(v).unwrap());
        let roots = vm.heap.root_count_for_test();
        let calls = vm.active_call_roots.len();
        let result = vm.iterator_next(&program, iterator).unwrap();
        let value_atom = vm.intern_atom("value");
        let value = vm.own_property(result, value_atom).unwrap();
        let tag = vm.intern_atom("tag");
        assert_eq!(vm.own_property(value, tag).unwrap().as_number(), Some(42.0));
        assert_eq!(vm.heap.weak_value(handles[0]), Some(source));
        assert_eq!(vm.heap.root_count_for_test(), roots);
        assert_eq!(vm.active_call_roots.len(), calls);
        assert!(
            matches!(vm.heap.iterator(iterator),Some(crate::heap::IteratorRecord {source,index:1,done:true,..}) if source.is_undefined())
        );
        vm.collect_now(&program);
        assert!(vm.heap.weak_value(handles[0]).is_none());
        assert_eq!(vm.heap.weak_value(handles[1]), Some(iterator));
        vm.set_property(vm.realm.globals, atom, Value::UNDEFINED)
            .unwrap();
        vm.collect_now(&program);
        assert!(vm.heap.weak_value(handles[1]).is_none());
    }
}

#[test]
fn completed_typed_array_iterators_release_source_and_backing() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for native in [
            Native::Uint8ArrayKeys,
            Native::Uint8ArrayValues,
            Native::Uint8ArrayEntries,
        ] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let program = compile(
                "function source() {return new Uint8Array([42]);}",
                "typed-iterator-release.js",
            )
            .unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("source");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let source = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let buffer = match vm.heap.get(source) {
                Some(super::Cell::TypedArray { buffer, .. }) => *buffer,
                _ => panic!("typed source"),
            };
            let iterator = vm.array_iterator_native(&program, native, source).unwrap();
            let handles = [iterator, source, buffer].map(|v| vm.heap.weak_handle(v).unwrap());
            let owner = vm.heap.root(iterator);
            vm.collect_now(&program);
            for (handle, value) in handles.iter().zip([iterator, source, buffer]) {
                assert_eq!(vm.heap.weak_value(*handle), Some(value));
            }
            vm.iterator_next(&program, iterator).unwrap();
            let result = vm.iterator_next(&program, iterator).unwrap();
            let atom = vm.intern_atom("done");
            assert_eq!(vm.own_property(result, atom), Some(Value::TRUE));
            vm.collect_now(&program);
            assert_eq!(vm.heap.weak_value(handles[0]), Some(iterator));
            assert!(vm.heap.weak_value(handles[1]).is_none());
            assert!(vm.heap.weak_value(handles[2]).is_none());
            let result = vm.iterator_next(&program, iterator).unwrap();
            assert_eq!(vm.own_property(result, atom), Some(Value::TRUE));
            vm.heap.release_root(owner);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(handles[0]).is_none());
        }
    }
}

#[test]
fn iterator_close_retains_forwarded_wrappers_and_restores_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for wrapper in ["raw", "protocol", "async-from-sync"] {
            for phase in [
                "success",
                "getter",
                "call",
                "primitive",
                "method",
                "absent",
                "null",
            ] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    var log=[];function readLog() {{return log.join(',');}}
                    function close() {{log.push('call:'+this.tag);$262.gc();if ('{phase}'==='call') throw {{kind:'call'}};
                        return '{phase}'==='primitive'?42:{{tag:this.tag}};
                    }}
                    function source() {{return {{tag:42,next() {{return {{done:true}};}},get return() {{
                        log.push('get:'+this.tag);$262.gc();if ('{phase}'==='getter') throw {{kind:'getter'}};
                        return '{phase}'==='absent'?undefined:'{phase}'==='null'?null:'{phase}'==='method'?42:close;
                    }}}};}}
                "#
                );
                let program = compile(&source, "iterator-close-ownership.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("source");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let source = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let iterator = match wrapper {
                    "raw" => source,
                    "protocol" => vm.iterator_from(&program, &[source]).unwrap(),
                    _ => vm.heap.alloc_iterator(crate::heap::IteratorRecord {
                        object: Vm::<TestHost>::empty_object(
                            vm.async_from_sync_iterator_proto,
                        ),
                        source,
                        kind: super::IteratorKind::AsyncFromSync,
                        index: 0,
                        done: false,
                        ext: crate::heap::IteratorExt {
                            position_high: 0,
                            next_method: None,
                            helper: None,
                            helper_running: false,
                            helper_started: false,
                            generator: None,
                        },
                    }),
                };
                let handles = [source, iterator].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.iterator_close(&program, iterator);

                assert_eq!(
                    result.is_ok(),
                    matches!(phase, "success" | "absent" | "null")
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([source, iterator]) {
                    assert_eq!(
                        vm.heap.weak_value(*handle),
                        Some(value),
                        "{wrapper} {phase}"
                    );
                }
                let log = vm.with_call_roots(
                    result
                        .iter()
                        .copied()
                        .chain(result.as_ref().err().and_then(|error| error.thrown_value())),
                    |vm| {
                        let atom = vm.intern_atom("readLog");
                        let reader = vm.own_property(vm.realm.globals, atom).unwrap();
                        let log = vm
                            .call_value(&program, reader, Value::UNDEFINED, &[])
                            .unwrap();
                        vm.to_string(&program, log).unwrap()
                    },
                );
                assert_eq!(
                    log,
                    if matches!(phase, "success" | "call" | "primitive") {
                        "get:42,call:42"
                    } else {
                        "get:42"
                    },
                    "{wrapper} {phase}"
                );
                let result_handle = match result {
                    Ok(result) if phase == "success" => {
                        let atom = vm.intern_atom("tag");
                        assert_eq!(
                            vm.own_property(result, atom).unwrap().as_number(),
                            Some(42.0)
                        );
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Ok(result) => {
                        assert!(result.is_undefined());
                        None
                    }
                    Err(error) if matches!(phase, "getter" | "call") => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        None
                    }
                    Err(error) => {
                        assert!(error.to_string().contains(if phase == "method" {
                            "not callable"
                        } else {
                            "not an object"
                        }));
                        None
                    }
                };
                vm.collect_now(&program);
                for handle in handles.into_iter().chain(result_handle) {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn regexp_match_all_creation_roots_fresh_species_matcher_and_inputs() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in ["success", "flags", "construct", "index", "setter"] {
            let mut vm = Vm::new(TestHost::default().with_test262());
            let source = format!(
                r#"
                function speciesFactory() {{return function Species() {{$262.gc();if ('{phase}'==='construct') throw {{kind:'construct'}};return matcherFactory();}};}}
                function matcherFactory() {{return {{exec() {{return null;}},set lastIndex(value) {{$262.gc();if ('{phase}'==='setter') throw {{kind:'setter'}};}}}};}}
                function source() {{return {{
                    get constructor() {{$262.gc();return {{[Symbol.species]:speciesFactory()}};}},
                    get flags() {{$262.gc();if ('{phase}'==='flags') throw {{kind:'flags'}};return 'g';}},
                    get lastIndex() {{$262.gc();if ('{phase}'==='index') throw {{kind:'index'}};return {{[Symbol.toPrimitive]() {{$262.gc();return 0;}}}};}}
                }};}}
            "#
            );
            let program = compile(&source, "match-all-creation-roots.js").unwrap();
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("source");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let receiver = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let input = vm.heap.alloc_string("a".into());
            let handles = [receiver, input].map(|v| vm.heap.weak_handle(v).unwrap());
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.regexp_symbol_match_all(&program, receiver, &[input]);
            assert_eq!(result.is_ok(), phase == "success", "{phase}");
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            for (handle, value) in handles.iter().zip([receiver, input]) {
                assert_eq!(vm.heap.weak_value(*handle), Some(value), "{phase}");
            }
            match result {
                Ok(iterator) => {
                    let matcher = match vm.heap.iterator(iterator) {
                        Some(crate::heap::IteratorRecord { source, .. }) => *source,
                        _ => panic!("regexp iterator"),
                    };
                    assert!(vm.is_object_like(matcher));
                    let matcher_handle = vm.heap.weak_handle(matcher).unwrap();
                    let owner = vm.heap.root(iterator);
                    vm.collect_now(&program);
                    assert_eq!(vm.heap.weak_value(matcher_handle), Some(matcher));
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                    vm.heap.release_root(owner);
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(matcher_handle).is_none());
                }
                Err(error) => {
                    let atom = vm.intern_atom("kind");
                    let kind = vm
                        .own_property(error.thrown_value().unwrap(), atom)
                        .unwrap();
                    assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_iterator_advance_roots_fresh_exec_result_and_restores_scopes() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (full, phases) in [
            (false, &["success"][..]),
            (
                true,
                &[
                    "success", "exec", "match", "coercion", "index", "numeric", "setter", "done",
                ][..],
            ),
        ] {
            for &phase in phases {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let input_code = if full { 55296 } else { 97 };
                let source = format!(
                    r#"
                function execute(input) {{$262.gc();if (input.charCodeAt(0)!=={input_code}) throw {{kind:'input'}};return '{phase}'==='done'?null:resultFactory();}}
                function resultFactory() {{return {{tag:42,get 0() {{$262.gc();if ('{phase}'==='match') throw {{kind:'match'}};
                    return {full}?{{[Symbol.toPrimitive]() {{$262.gc();if ('{phase}'==='coercion') throw {{kind:'coercion'}};return '';}}}}:'';
                }}}};}}
                function numeric() {{return {{[Symbol.toPrimitive]() {{$262.gc();if ('{phase}'==='numeric') throw {{kind:'numeric'}};return 0;}}}};}}
                function matcher() {{if (!{full}) return {{exec:execute,lastIndex:{{valueOf() {{$262.gc();return 0;}}}}}};return {{
                    get exec() {{$262.gc();if ('{phase}'==='exec') throw {{kind:'exec'}};return execute;}},
                    get lastIndex() {{$262.gc();if ('{phase}'==='index') throw {{kind:'index'}};return numeric();}},
                    set lastIndex(value) {{$262.gc();if ('{phase}'==='setter') throw {{kind:'setter'}};}}
                }};}}
            "#
                );
                let program = compile(&source, "regexp-iterator-advance-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("matcher");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let matcher = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let iterator = vm.heap.alloc_iterator(crate::heap::IteratorRecord {
                    object: Vm::<TestHost>::empty_object(
                        vm.regexp_string_iterator_proto,
                    ),
                    source: matcher,
                    kind: super::IteratorKind::RegExpStringMatchAll,
                    index: 0,
                    done: false,
                    ext: crate::heap::IteratorExt {
                        position_high: 0,
                        next_method: None,
                        helper: Some(Box::new(
                            crate::heap::IteratorHelper::RegExpStringMatchAll {
                                input: JsString::from_units(if full {
                                    &[0xd800, 97]
                                } else {
                                    &[97]
                                }),
                                global: true,
                                unicode: true,
                            },
                        )),
                        helper_running: false,
                        helper_started: false,
                        generator: None,
                    },
                });
                let handles = [matcher, iterator].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let result = vm.iterator_next(&program, iterator);
                assert_eq!(
                    result.is_ok(),
                    matches!(phase, "success" | "done"),
                    "{phase}"
                );
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let result_handle = match result {
                    Ok(result) => {
                        let done = vm.intern_atom("done");
                        assert_eq!(
                            vm.own_property(result, done),
                            Some(if phase == "done" {
                                Value::TRUE
                            } else {
                                Value::FALSE
                            })
                        );
                        if phase != "done" {
                            let atom = vm.intern_atom("value");
                            let matched = vm.own_property(result, atom).unwrap();
                            let tag = vm.intern_atom("tag");
                            assert_eq!(
                                vm.own_property(matched, tag).unwrap().as_number(),
                                Some(42.0)
                            );
                        }
                        Some(vm.heap.weak_handle(result).unwrap())
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                        None
                    }
                };
                for (handle, value) in handles.iter().zip([matcher, iterator]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value));
                }
                assert!(
                    matches!(vm.heap.iterator(iterator),Some(crate::heap::IteratorRecord {done,..}) if *done==(phase=="done"))
                );
                vm.collect_now(&program);
                for handle in handles.into_iter().chain(result_handle) {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn regexp_split_roots_inputs_matcher_results_and_accumulated_captures() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "limit",
            "set",
            "exec-get",
            "exec",
            "index",
            "index-number",
            "length",
            "length-number",
            "capture1",
            "capture2",
            "second-exec",
        ] {
            for abrupt in [false, true] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function hit(name) {{if (name==='{phase}') {{$262.gc();if ({abrupt}) throw {{kind:name}};}}}}
                    function capture(tag) {{return {{tag:tag}};}}
                    function result() {{return {{
                        get length() {{hit('length');return {{valueOf() {{hit('length-number');return 3;}}}};}},
                        get 1() {{hit('capture1');return capture(42);}},
                        get 2() {{hit('capture2');return capture(43);}}
                    }};}}
                    function matcher() {{var index=0,matched=0;return {{
                        set lastIndex(value) {{hit('set');index=value;}},
                        get lastIndex() {{hit('index');return {{valueOf() {{hit('index-number');return index;}}}};}},
                        get exec() {{hit('exec-get');return function(input) {{hit('exec');if (input!=='a,b,c') throw {{kind:'input'}};if (index===1||index===3) {{if (matched++) hit('second-exec');index++;return result();}}return null;}};}}
                    }};}}
                    function receiver() {{return {{constructor:{{[Symbol.species]:function Species() {{return matcher();}}}},flags:'g'}};}}
                    function limit() {{return {{valueOf() {{hit('limit');return 100;}}}};}}
                "#
                );
                let program = compile(&source, "regexp-split-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let factory = vm.intern_atom("receiver");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let receiver = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let input = vm.heap.alloc_string("a,b,c".into());
                let receiver_root = vm.heap.root(receiver);
                let input_root = vm.heap.root(input);
                let factory = vm.intern_atom("limit");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let limit = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                vm.heap.release_root(receiver_root);
                vm.heap.release_root(input_root);
                let handles = [receiver, input, limit].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let outcome = vm.regexp_symbol_split(&program, receiver, &[input, limit]);
                assert_eq!(outcome.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([receiver, input, limit]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value), "{phase}/{abrupt}");
                }
                match outcome {
                    Ok(array) => {
                        let array_root = vm.heap.root(array);
                        vm.collect_now(&program);
                        for (index, expected) in [(0, "a"), (3, "b"), (6, "c")] {
                            let value = vm
                                .get_index(&program, array, Value::number(index as f64))
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), expected, "{phase}");
                        }
                        for (index, expected) in [(1, 42.0), (2, 43.0), (4, 42.0), (5, 43.0)] {
                            let value = vm
                                .get_index(&program, array, Value::number(index as f64))
                                .unwrap();
                            let tag = vm.intern_atom("tag");
                            assert_eq!(
                                vm.own_property(value, tag).and_then(Value::as_number),
                                Some(expected),
                                "{phase}/{index}"
                            );
                        }
                        vm.heap.release_root(array_root);
                    }
                    Err(error) => {
                        let kind = vm.intern_atom("kind");
                        let kind = vm
                            .own_property(error.thrown_value().unwrap(), kind)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, kind).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none(), "{phase}/{abrupt}");
                }
            }
        }
    }
}

#[test]
fn regexp_search_roots_saved_index_and_fresh_result_across_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "input", "previous", "reset", "exec-get", "exec", "current", "restore", "index",
        ] {
            for abrupt in [false, true] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function previous() {{return {{tag:41,valueOf() {{throw {{kind:'coerced-previous'}};}}}};}}
                    function result() {{return {{get index() {{hit('index');return 43;}}}};}}
                    function receiver() {{var reads=0;return {{
                        get lastIndex() {{if(reads++===0) {{hit('previous');return previous();}}hit('current');return 1;}},
                        set lastIndex(value) {{if(value===0)hit('reset');else {{hit('restore');if(value.tag!==41)throw {{kind:'previous-value'}};}}}},
                        get exec() {{hit('exec-get');return function(input) {{hit('exec');if(input!=='a')throw {{kind:'input-value'}};return result();}};}}
                    }};}}
                    function input() {{return {{[Symbol.toPrimitive](hint) {{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a';}}}};}}
                "#
                );
                let program = compile(&source, "regexp-search-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let factory = vm.intern_atom("receiver");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let receiver = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let receiver_root = vm.heap.root(receiver);
                let factory = vm.intern_atom("input");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let input = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                vm.heap.release_root(receiver_root);
                let handles = [receiver, input].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let outcome = vm.regexp_symbol_search(&program, receiver, &[input]);
                assert_eq!(outcome.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([receiver, input]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value), "{phase}/{abrupt}");
                }
                match outcome {
                    Ok(value) => assert_eq!(value.as_number(), Some(43.0), "{phase}"),
                    Err(error) => {
                        let kind = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), kind)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none(), "{phase}/{abrupt}");
                }
            }
        }
    }
}

#[test]
fn regexp_match_roots_input_and_accumulated_strings_across_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "input",
            "flags",
            "flags-string",
            "reset",
            "exec-get",
            "exec",
            "match",
            "match-string",
            "index",
            "index-number",
            "advance",
            "second-exec",
        ] {
            for abrupt in [false, true] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function result(text) {{return {{get 0() {{hit('match');return {{[Symbol.toPrimitive](hint) {{hit('match-string');if(hint!=='string')throw {{kind:'match-hint'}};return text;}}}};}}}};}}
                    function receiver() {{var index=0,writes=0,calls=0;return {{
                        get flags() {{hit('flags');return {{[Symbol.toPrimitive](hint) {{hit('flags-string');if(hint!=='string')throw {{kind:'flags-hint'}};return 'g';}}}};}},
                        set lastIndex(value) {{hit(writes++===0?'reset':'advance');index=value;}},
                        get lastIndex() {{hit('index');return {{[Symbol.toPrimitive](hint) {{hit('index-number');if(hint!=='number')throw {{kind:'index-hint'}};return index;}}}};}},
                        get exec() {{hit('exec-get');return function(input) {{hit('exec');if(input!=='a')throw {{kind:'input-value'}};calls++;if(calls===4) {{hit('second-exec');return null;}}return result(calls===1?'A':calls===2?'':'B');}};}}
                    }};}}
                    function input() {{return {{[Symbol.toPrimitive](hint) {{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a';}}}};}}
                "#
                );
                let program = compile(&source, "regexp-match-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let factory = vm.intern_atom("receiver");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let receiver = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let receiver_root = vm.heap.root(receiver);
                let factory = vm.intern_atom("input");
                let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                let input = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                vm.heap.release_root(receiver_root);
                let handles = [receiver, input].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let outcome = vm.regexp_symbol_match(&program, receiver, &[input]);
                assert_eq!(outcome.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([receiver, input]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value), "{phase}/{abrupt}");
                }
                match outcome {
                    Ok(array) => {
                        let root = vm.heap.root(array);
                        vm.collect_now(&program);
                        for (index, text) in ["A", "", "B"].iter().enumerate() {
                            let value = vm
                                .get_index(&program, array, Value::number(index as f64))
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                *text,
                                "{phase}/{index}"
                            );
                        }
                        vm.heap.release_root(root);
                    }
                    Err(error) => {
                        let kind = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), kind)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none(), "{phase}/{abrupt}");
                }
            }
        }
    }
}

#[test]
fn string_match_search_custom_dispatch_roots_original_inputs() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (native, name) in [
            (super::Native::StringMatch, "match"),
            (super::Native::StringSearch, "search"),
        ] {
            for phase in ["get", "call"] {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                        function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function selected() {{return function(arg) {{hit('call');if(this.tag!==43||arg.tag!==42)throw {{kind:'identity'}};return {{tag:44,argument:arg,receiver:this}};}};}}
                        function receiver() {{return {{tag:42,[Symbol.toPrimitive]() {{throw {{kind:'coerced'}};}}}};}}
                        function pattern() {{return {{tag:43,get [Symbol.{name}]() {{hit('get');return selected();}}}};}}
                    "#
                    );
                    let program = compile(&source, "string-custom-dispatch-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let factory = vm.intern_atom("receiver");
                    let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                    let receiver = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let root = vm.heap.root(receiver);
                    let factory = vm.intern_atom("pattern");
                    let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                    let pattern = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    vm.heap.release_root(root);
                    let handles = [receiver, pattern].map(|v| vm.heap.weak_handle(v).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome =
                        vm.string_match_or_search_native(&program, native, receiver, &[pattern]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{name}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip([receiver, pattern]) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(value));
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            for (key, expected) in [("argument", receiver), ("receiver", pattern)] {
                                let atom = vm.intern_atom(key);
                                assert_eq!(vm.own_property(value, atom), Some(expected));
                            }
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_match_search_fallback_roots_converted_input_and_fresh_matcher() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (native, name) in [
            (super::Native::StringMatch, "match"),
            (super::Native::StringSearch, "search"),
        ] {
            for phase in ["input", "pattern", "method", "call"] {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                        function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function selected() {{return function(input) {{hit('call');if(input.charCodeAt(0)!==55296||this.source!=='a')throw {{kind:'values'}};return {{tag:44,matcher:this,input:input}};}};}}
                        Object.defineProperty(RegExp.prototype,Symbol.{name},{{configurable:true,get() {{hit('method');return selected();}}}});
                        function receiver() {{return {{[Symbol.toPrimitive](hint) {{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return String.fromCharCode(55296,97);}}}};}}
                        function pattern() {{return {{[Symbol.{name}]:null,[Symbol.toPrimitive](hint) {{hit('pattern');if(hint!=='string')throw {{kind:'pattern-hint'}};return 'a';}}}};}}
                    "#
                    );
                    let program = compile(&source, "string-fallback-dispatch-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let factory = vm.intern_atom("receiver");
                    let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                    let receiver = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let root = vm.heap.root(receiver);
                    let factory = vm.intern_atom("pattern");
                    let factory = vm.own_property(vm.realm.globals, factory).unwrap();
                    let pattern = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    vm.heap.release_root(root);
                    let handles = [receiver, pattern].map(|v| vm.heap.weak_handle(v).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome =
                        vm.string_match_or_search_native(&program, native, receiver, &[pattern]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{name}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip([receiver, pattern]) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(value));
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("matcher");
                            let matcher = vm.own_property(value, atom).unwrap();
                            assert!(
                                matches!(vm.heap.get(matcher),Some(super::Cell::RegExp{meta,..})if meta.source.host_string()=="a")
                            );
                            let atom = vm.intern_atom("input");
                            let input = vm.own_property(value, atom).unwrap();
                            assert!(
                                vm.heap.string(input).is_some_and(|text| text.units()==[0xd800,97])
                            );
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_match_all_custom_dispatch_roots_flags_and_original_receiver() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in ["is-regexp", "flags", "flags-string", "method", "call"] {
            for abrupt in [false, true] {
                let mut vm = Vm::new(TestHost::default().with_test262());
                let source = format!(
                    r#"
                    function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function selected() {{return function(arg) {{hit('call');if(this.tag!==43||arg.tag!==42)throw {{kind:'identity'}};return {{tag:44,argument:arg,receiver:this}};}};}}
                    function receiver() {{return {{tag:42,[Symbol.toPrimitive]() {{throw {{kind:'coerced'}};}}}};}}
                    function pattern() {{return {{tag:43,
                        get [Symbol.match]() {{hit('is-regexp');return true;}},
                        get flags() {{hit('flags');return {{[Symbol.toPrimitive](hint) {{hit('flags-string');if(hint!=='string')throw {{kind:'flags-hint'}};return 'g';}}}};}},
                        get [Symbol.matchAll]() {{hit('method');return selected();}}
                    }};}}
                "#
                );
                let program = compile(&source, "string-match-all-custom-roots.js").unwrap();
                vm.execute(&program).unwrap();
                let atom = vm.intern_atom("receiver");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let receiver = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                let root = vm.heap.root(receiver);
                let atom = vm.intern_atom("pattern");
                let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                let pattern = vm
                    .call_value(&program, factory, Value::UNDEFINED, &[])
                    .unwrap();
                vm.heap.release_root(root);
                let handles = [receiver, pattern].map(|v| vm.heap.weak_handle(v).unwrap());
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let outcome = vm.string_match_all_native(&program, receiver, &[pattern]);
                assert_eq!(outcome.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                for (handle, value) in handles.iter().zip([receiver, pattern]) {
                    assert_eq!(vm.heap.weak_value(*handle), Some(value));
                }
                match outcome {
                    Ok(value) => {
                        let root = vm.heap.root(value);
                        vm.collect_now(&program);
                        for (key, expected) in [("argument", receiver), ("receiver", pattern)] {
                            let atom = vm.intern_atom(key);
                            assert_eq!(vm.own_property(value, atom), Some(expected));
                        }
                        vm.heap.release_root(root);
                    }
                    Err(error) => {
                        let atom = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), atom)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
                for handle in handles {
                    assert!(vm.heap.weak_value(handle).is_none());
                }
            }
        }
    }
}

#[test]
fn string_match_all_fallback_roots_input_and_intrinsic_matcher() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, phases) in [
            ("object", &["input", "pattern", "method", "call"][..]),
            ("primitive", &["input", "method", "call"][..]),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                        function hit(name) {{if(name==='{phase}') {{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function selected() {{return function(input) {{hit('call');if(input.charCodeAt(0)!==55296||this.source!=='a'||this.flags!=='g')throw {{kind:'values'}};return {{tag:44,matcher:this,input:input}};}};}}
                        Object.defineProperty(RegExp.prototype,Symbol.matchAll,{{configurable:true,get() {{hit('method');return selected();}}}});
                        function receiver() {{return {{[Symbol.toPrimitive](hint) {{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return String.fromCharCode(55296,97);}}}};}}
                        function pattern() {{return {{[Symbol.match]:false,[Symbol.matchAll]:null,[Symbol.toPrimitive](hint) {{hit('pattern');if(hint!=='string')throw {{kind:'pattern-hint'}};return 'a';}}}};}}
                    "#
                    );
                    let program = compile(&source, "string-match-all-fallback-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let atom = vm.intern_atom("receiver");
                    let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                    let receiver = vm
                        .call_value(&program, factory, Value::UNDEFINED, &[])
                        .unwrap();
                    let root = vm.heap.root(receiver);
                    let pattern = if kind == "object" {
                        let atom = vm.intern_atom("pattern");
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        vm.call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap()
                    } else {
                        vm.heap.alloc_string("a".into())
                    };
                    vm.heap.release_root(root);
                    let handles = [receiver, pattern].map(|v| vm.heap.weak_handle(v).unwrap());
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.string_match_all_native(&program, receiver, &[pattern]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip([receiver, pattern]) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(value));
                    }
                    match outcome {
                        Ok(value) => {
                            let root = vm.heap.root(value);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("matcher");
                            let matcher = vm.own_property(value, atom).unwrap();
                            assert!(
                                matches!(vm.heap.get(matcher),Some(super::Cell::RegExp{meta,..})if meta.source.host_string()=="a"&&meta.flags.text()=="g")
                            );
                            let atom = vm.intern_atom("input");
                            let input = vm.own_property(value, atom).unwrap();
                            assert!(
                                vm.heap.string(input).is_some_and(|text| text.units()==[0xd800,97])
                            );
                            vm.heap.release_root(root);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_replace_protocol_roots_receiver_search_and_replacement() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (all, phases) in [
            (false, &["method", "call"][..]),
            (
                true,
                &["is-regexp", "flags", "flags-string", "method", "call"][..],
            ),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function selected(){{return function(input,replacement){{hit('call');if(this.tag!==43||input.tag!==42||replacement.tag!==44)throw {{kind:'identity'}};return {{input:input,search:this,replacement:replacement}};}};}}
                    function receiver(){{return {{tag:42,[Symbol.toPrimitive](){{throw {{kind:'coerced'}};}}}};}}
                    function replacement(){{return {{tag:44,[Symbol.toPrimitive](){{throw {{kind:'coerced'}};}}}};}}
                    function search(){{return {{tag:43,get [Symbol.match](){{hit('is-regexp');return true;}},get flags(){{hit('flags');return {{[Symbol.toPrimitive](hint){{hit('flags-string');return 'g';}}}};}},get [Symbol.replace](){{hit('method');return selected();}}}};}}
                "#
                    );
                    let program = compile(&source, "string-replace-protocol-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["receiver", "search", "replacement"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|v| vm.heap.weak_handle(*v).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.string_replace_native(&program, values[0], &values[1..], all);
                    assert_eq!(outcome.is_ok(), !abrupt, "{all}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            for (key, expected) in
                                ["input", "search", "replacement"].iter().zip(&values)
                            {
                                let atom = vm.intern_atom(key);
                                assert_eq!(vm.own_property(value, atom), Some(*expected));
                            }
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn string_replace_fallback_roots_conversions_and_repeated_callback_input() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for all in [false, true] {
            for kind in ["template", "empty", "nonempty"] {
                let phases = if kind == "template" {
                    &["input", "search", "replacement"][..]
                } else if all {
                    &["input", "search", "call", "return-string", "second-call"][..]
                } else {
                    &["input", "search", "call", "return-string"][..]
                };
                for phase in phases {
                    for abrupt in [false, true] {
                        let mut vm = Vm::new(TestHost::default().with_test262());
                        let source = format!(
                            r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function receiver(){{return {{[Symbol.toPrimitive](hint){{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a-a';}}}};}}
                    function search(){{return {{[Symbol.match]:false,[Symbol.replace]:null,[Symbol.toPrimitive](hint){{hit('search');if(hint!=='string')throw {{kind:'search-hint'}};return '{kind}'==='empty'?'':'a';}}}};}}
                    function replacement(){{if('{kind}'==='template')return {{[Symbol.toPrimitive](hint){{hit('replacement');if(hint!=='string')throw {{kind:'replacement-hint'}};return 'X';}}}};
                        var calls=0;return function(matched,index,input){{hit('call');if(calls++===1)hit('second-call');if(input!=='a-a'||matched!==('{kind}'==='empty'?'':'a'))throw {{kind:'arguments'}};return {{[Symbol.toPrimitive](hint){{hit('return-string');if(hint!=='string')throw {{kind:'return-hint'}};return 'X';}}}};}};}}
                "#
                        );
                        let program = compile(&source, "string-replace-fallback-roots.js").unwrap();
                        vm.execute(&program).unwrap();
                        let mut values = Vec::new();
                        let mut owners = Vec::new();
                        for name in ["receiver", "search", "replacement"] {
                            let atom = vm.intern_atom(name);
                            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                            let value = vm
                                .call_value(&program, factory, Value::UNDEFINED, &[])
                                .unwrap();
                            values.push(value);
                            owners.push(vm.heap.root(value));
                        }
                        for owner in owners {
                            vm.heap.release_root(owner);
                        }
                        let handles = values
                            .iter()
                            .map(|v| vm.heap.weak_handle(*v).unwrap())
                            .collect::<Vec<_>>();
                        let roots = vm.heap.root_count_for_test();
                        let calls = vm.active_call_roots.len();
                        let outcome =
                            vm.string_replace_native(&program, values[0], &values[1..], all);
                        assert_eq!(outcome.is_ok(), !abrupt, "{all}/{kind}/{phase}/{abrupt}");
                        assert_eq!(vm.heap.root_count_for_test(), roots);
                        assert_eq!(vm.active_call_roots.len(), calls);
                        for (handle, value) in handles.iter().zip(&values) {
                            assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                        }
                        match outcome {
                            Ok(value) => assert_eq!(
                                vm.to_string(&program, value).unwrap(),
                                match (all, kind) {
                                    (false, "empty") => "Xa-a",
                                    (true, "empty") => "XaX-XaX",
                                    (false, _) => "X-a",
                                    (true, _) => "X-X",
                                },
                                "{all}/{kind}/{phase}"
                            ),
                            Err(error) => {
                                let atom = vm.intern_atom("kind");
                                let value = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                            }
                        }
                        vm.collect_now(&program);
                        for handle in handles {
                            assert!(vm.heap.weak_value(handle).is_none());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn string_split_roots_protocol_and_conversion_inputs() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (custom, phases) in [
            (true, &["method", "call"][..]),
            (false, &["method", "input", "limit", "separator"][..]),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function receiver(){{return {{tag:42,[Symbol.toPrimitive](hint){{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a-a';}}}};}}
                    function limit(){{return {{tag:44,[Symbol.toPrimitive](hint){{hit('limit');if(hint!=='number')throw {{kind:'limit-hint'}};return 2;}}}};}}
                    function separator(){{return {{tag:43,get [Symbol.split](){{hit('method');if(!{custom})return null;return function(input,limit){{hit('call');if(this.tag!==43||input.tag!==42||limit.tag!==44)throw {{kind:'identity'}};return {{input:input,separator:this,limit:limit}};}};}},[Symbol.toPrimitive](hint){{hit('separator');if(hint!=='string')throw {{kind:'separator-hint'}};return '-';}}}};}}
                "#
                    );
                    let program = compile(&source, "string-split-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["receiver", "separator", "limit"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|v| vm.heap.weak_handle(*v).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.string_split_native(&program, values[0], &values[1..]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{custom}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            if custom {
                                for (key, expected) in
                                    ["input", "separator", "limit"].iter().zip(&values)
                                {
                                    let atom = vm.intern_atom(key);
                                    assert_eq!(vm.own_property(value, atom), Some(*expected));
                                }
                            } else {
                                let atom = vm.intern_atom("join");
                                let join = vm.get_property(&program, value, atom).unwrap();
                                let joined = vm.call_value(&program, join, value, &[]).unwrap();
                                assert_eq!(vm.to_string(&program, joined).unwrap(), "a,a");
                            }
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_replace_roots_inputs_results_and_converted_captures() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (callable, phases) in [
            (
                false,
                &[
                    "input",
                    "replacement",
                    "flags",
                    "flags-string",
                    "set-index",
                    "exec",
                    "matched",
                    "matched-string",
                    "length",
                    "length-number",
                    "index",
                    "index-number",
                    "capture",
                    "capture-string",
                    "groups",
                    "group",
                    "group-string",
                ][..],
            ),
            (
                true,
                &[
                    "input",
                    "flags",
                    "flags-string",
                    "set-index",
                    "exec",
                    "matched",
                    "matched-string",
                    "length",
                    "length-number",
                    "index",
                    "index-number",
                    "capture",
                    "capture-string",
                    "groups",
                    "call",
                    "return-string",
                ][..],
            ),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function input(){{return {{[Symbol.toPrimitive](hint){{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'a-a';}}}};}}
                    function replacement(){{if(!{callable})return {{[Symbol.toPrimitive](hint){{hit('replacement');return '$<x>';}}}};return function(matched,capture,missing,index,input,groups){{hit('call');if(matched!=='a'||capture!=='C'||missing!==undefined||index!==0||input!=='a-a'||groups.tag!==44)throw {{kind:'arguments'}};return {{[Symbol.toPrimitive](hint){{hit('return-string');return 'X';}}}};}};}}
                    function receiver(){{var calls=0;return {{get flags(){{hit('flags');return {{[Symbol.toPrimitive](hint){{hit('flags-string');return 'g';}}}};}},set lastIndex(value){{hit('set-index');}},exec(input){{hit('exec');if(input!=='a-a')throw {{kind:'exec-input'}};if(calls++>0)return null;return {{get 0(){{hit('matched');return {{[Symbol.toPrimitive](hint){{hit('matched-string');return 'a';}}}};}},get length(){{hit('length');return {{[Symbol.toPrimitive](hint){{hit('length-number');if(hint!=='number')throw {{kind:'length-hint'}};return 3;}}}};}},get index(){{hit('index');return {{[Symbol.toPrimitive](hint){{hit('index-number');if(hint!=='number')throw {{kind:'index-hint'}};return 0;}}}};}},get 1(){{hit('capture');return {{[Symbol.toPrimitive](hint){{hit('capture-string');return 'C';}}}};}},get groups(){{hit('groups');return {{tag:44,get x(){{hit('group');return {{[Symbol.toPrimitive](hint){{hit('group-string');return 'X';}}}};}}}};}}}};}}}};}}
                "#
                    );
                    let program = compile(&source, "regexp-replace-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["receiver", "input", "replacement"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|v| vm.heap.weak_handle(*v).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.regexp_symbol_replace(&program, values[0], &values[1..]);
                    assert_eq!(outcome.is_ok(), !abrupt, "{callable}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                    }
                    match outcome {
                        Ok(value) => assert_eq!(vm.to_string(&program, value).unwrap(), "X-a"),
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_construction_roots_protocol_inputs_and_initialization_projections() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, phases) in [
            ("identity", &["is-regexp", "constructor"][..]),
            (
                "observable-call",
                &[
                    "is-regexp",
                    "constructor",
                    "source",
                    "flags",
                    "source-string",
                    "flags-string",
                ][..],
            ),
            (
                "observable-new",
                &[
                    "is-regexp",
                    "source",
                    "flags",
                    "prototype",
                    "source-string",
                    "flags-string",
                ][..],
            ),
            (
                "raw-new",
                &["is-regexp", "prototype", "source-string", "flags-string"][..],
            ),
            (
                "internal-new",
                &["is-regexp", "prototype", "flags-string"][..],
            ),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                        function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function text(name,value){{return {{[Symbol.toPrimitive](hint){{hit(name);if(hint!=='string')throw {{kind:'hint'}};return value;}}}};}}
                        function pattern(){{if('{kind}'==='internal-new'){{var value=/a/g;Object.defineProperty(value,Symbol.match,{{get(){{hit('is-regexp');return false;}}}});return value;}}
                            return {{get [Symbol.match](){{hit('is-regexp');return '{kind}'!=='raw-new';}},get constructor(){{hit('constructor');return '{kind}'==='identity'?RegExp:null;}},get source(){{hit('source');return text('source-string','a');}},get flags(){{hit('flags');return text('flags-string','i');}},[Symbol.toPrimitive](hint){{hit('source-string');if(hint!=='string')throw {{kind:'hint'}};return 'a';}}}};}}
                        function flags(){{return text('flags-string','i');}}
                        function target(){{return new Proxy(function(){{}},{{get(object,key,receiver){{if(key==='prototype'){{hit('prototype');return {{tag:42}};}}return Reflect.get(object,key,receiver);}}}});}}
                    "#
                    );
                    let program = compile(&source, "regexp-construction-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let construct = kind.ends_with("new");
                    let explicit_flags = matches!(kind, "raw-new" | "internal-new");
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in [
                        Some("pattern"),
                        explicit_flags.then_some("flags"),
                        construct.then_some("target"),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|value| vm.heap.weak_handle(*value).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let args = if explicit_flags {
                        &values[..2]
                    } else {
                        &values[..1]
                    };
                    let target = construct.then(|| *values.last().unwrap());
                    let outcome = vm.construct_regexp_native(&program, args, target);
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(vm.heap.weak_value(*handle), Some(*value));
                    }
                    match outcome {
                        Ok(value) if kind == "identity" => assert_eq!(value, values[0]),
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            let Some(super::Cell::RegExp { object, meta, .. }) = vm.heap.get(value)
                            else {
                                panic!("constructor returns RegExp");
                            };
                            let (source, flags) = (&meta.source, meta.flags.text());
                            assert_eq!(source.units(), &[u16::from(b'a')]);
                            assert_eq!(flags, "i");
                            if construct {
                                let proto = object.proto();
                                let atom = vm.intern_atom("tag");
                                let tag = vm.own_property(proto, atom).unwrap();
                                assert_eq!(tag, Value::number(42.0));
                            }
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_entrypoints_root_receivers_and_arguments_through_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, phases) in [
            ("compile", &["input", "flags-string"][..]),
            (
                "format",
                &["source", "source-string", "flags", "flags-string"][..],
            ),
            (
                "flags",
                &[
                    "hasIndices",
                    "global",
                    "ignoreCase",
                    "multiline",
                    "dotAll",
                    "unicode",
                    "unicodeSets",
                    "sticky",
                ][..],
            ),
            ("exec", &["input", "last-index"][..]),
            ("test", &["input", "exec-get", "exec-call"][..]),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function text(name,value){{return {{[Symbol.toPrimitive](hint){{hit(name);if(hint!=='string')throw {{kind:'hint'}};return value;}}}};}}
                    function input(){{return text('input','a');}}
                    function flags(){{return text('flags-string','g');}}
                    function receiver(){{if('{kind}'==='compile')return /old/;
                        if('{kind}'==='exec'){{var value=/a/g;value.lastIndex={{[Symbol.toPrimitive](hint){{hit('last-index');if(hint!=='number')throw {{kind:'index-hint'}};return 0;}}}};return value;}}
                        if('{kind}'==='test')return {{get exec(){{hit('exec-get');return function(input){{hit('exec-call');if(input!=='a')throw {{kind:'input'}};return {{}};}};}}}};
                        if('{kind}'==='format')return {{get source(){{hit('source');return text('source-string','a');}},get flags(){{hit('flags');return text('flags-string','g');}}}};
                        var result={{}};for(var name of ['hasIndices','global','ignoreCase','multiline','dotAll','unicode','unicodeSets','sticky']){{(function(name){{Object.defineProperty(result,name,{{get(){{hit(name);return true;}}}});}})(name);}}return result;
                    }}
                "#
                    );
                    let program = compile(&source, "regexp-entrypoint-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in [
                        Some("receiver"),
                        matches!(kind, "compile" | "exec" | "test").then_some("input"),
                        (kind == "compile").then_some("flags"),
                    ]
                    .into_iter()
                    .flatten()
                    {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    let handles = values
                        .iter()
                        .map(|value| vm.heap.weak_handle(*value).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = match kind {
                        "compile" => vm.regexp_compile_native(&program, values[0], &values[1..]),
                        "format" => vm.regexp_to_string_native(&program, values[0]),
                        "flags" => vm.regexp_flags_native(&program, values[0]),
                        "exec" => vm.regexp_builtin_exec(&program, values[0], &values[1..]),
                        "test" => vm.regexp_test(&program, values[0], &values[1..]),
                        _ => unreachable!(),
                    };
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(
                            vm.heap.weak_value(*handle),
                            Some(*value),
                            "{kind}/{phase}/{abrupt}"
                        );
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            match kind {
                                "compile" => {
                                    assert_eq!(value, values[0]);
                                    let Some(super::Cell::RegExp { meta, .. }) = vm.heap.get(value)
                                    else {
                                        panic!("compiled receiver retained");
                                    };
                                    let (source, flags) = (&meta.source, meta.flags.text());
                                    assert_eq!(source.units(), &[u16::from(b'a')]);
                                    assert_eq!(flags, "g");
                                }
                                "format" => {
                                    assert_eq!(vm.to_string(&program, value).unwrap(), "/a/g")
                                }
                                "flags" => {
                                    assert_eq!(vm.to_string(&program, value).unwrap(), "dgimsuvy")
                                }
                                "test" => assert_eq!(value, Value::TRUE),
                                "exec" => {
                                    let matched =
                                        vm.get_index(&program, value, Value::number(0.0)).unwrap();
                                    assert_eq!(vm.to_string(&program, matched).unwrap(), "a");
                                }
                                _ => unreachable!(),
                            };
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn regexp_literal_sites_share_matchers_but_create_fresh_objects() {
    let program = Engine::specialize(
        r#"
        function make() { return /a/g; }
        var first = make();
        var second = make();
        if (first === second) throw new Error('literal object was cached');
        if (!first.exec('a') || first.lastIndex !== 1 || second.lastIndex !== 0)
            throw new Error('lastIndex was shared');
        first.compile('b', 'g');
        if (!first.test('b') || !second.test('a'))
            throw new Error('compile changed a shared matcher');
        var callbackMatcher = make();
        var nested = false;
        var replaced = 'a'.replace(callbackMatcher, function () {
            callbackMatcher.lastIndex = 0;
            nested = !!callbackMatcher.exec('a');
            return 'x';
        });
        if (!nested || replaced !== 'x') throw new Error('reentrant exec failed');
        "#,
        "regexp-literal-site.js",
    )
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    assert_eq!(
        vm.programs
            .regexp_literal_record_count(super::program_store::ProgramId::MAIN),
        1
    );
}

#[test]
fn native_dispatch_roots_raw_inputs_through_receiver_normalization_and_calls() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (native, kind, phases) in [
            (
                Native::StringSubstring,
                "substring",
                &["input", "first", "second"][..],
            ),
            (
                Native::StringConcat,
                "concat",
                &["input", "first", "second"][..],
            ),
            (Native::StringAnchor, "anchor", &["input", "first"][..]),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    for entry in ["direct", "guarded"] {
                        let mut vm = Vm::new(TestHost::default().with_test262());
                        let source = format!(
                            r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function receiver(){{return {{[Symbol.toPrimitive](hint){{hit('input');if(hint!=='string')throw {{kind:'input-hint'}};return 'abc';}}}};}}
                    function first(){{return {{[Symbol.toPrimitive](hint){{hit('first');if(hint!==('{kind}'==='substring'?'number':'string'))throw {{kind:'first-hint'}};return '{kind}'==='substring'?1:'X';}}}};}}
                    function second(){{return {{[Symbol.toPrimitive](hint){{hit('second');if(hint!==('{kind}'==='substring'?'number':'string'))throw {{kind:'second-hint'}};return '{kind}'==='substring'?2:'Y';}}}};}}
                "#
                        );
                        let program = compile(&source, "native-dispatch-roots.js").unwrap();
                        vm.execute(&program).unwrap();
                        let mut values = Vec::new();
                        let mut owners = Vec::new();
                        for name in [
                            Some("receiver"),
                            Some("first"),
                            (kind != "anchor").then_some("second"),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            let atom = vm.intern_atom(name);
                            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                            let value = vm
                                .call_value(&program, factory, Value::UNDEFINED, &[])
                                .unwrap();
                            values.push(value);
                            owners.push(vm.heap.root(value));
                        }
                        if kind != "substring" {
                            for owner in &owners {
                                vm.heap.release_root(*owner);
                            }
                        }
                        let handles = values
                            .iter()
                            .map(|value| vm.heap.weak_handle(*value).unwrap())
                            .collect::<Vec<_>>();
                        let roots = vm.heap.root_count_for_test();
                        let calls = vm.active_call_roots.len();
                        let active_native = vm.realm.promise.active_native.len();
                        let callee = (entry == "guarded").then(|| {
                            let function = vm.native_with_env(native, Value::UNDEFINED);
                            (function, vm.heap.weak_handle(function).unwrap())
                        });
                        let outcome = match callee {
                            Some((function, _)) => vm.call_native_guarded(
                                &program,
                                native,
                                values[0],
                                &values[1..],
                                function,
                            ),
                            None => vm.call_native(&program, native, values[0], &values[1..]),
                        };
                        assert_eq!(vm.realm.promise.active_native.len(), active_native);
                        if let Some((function, handle)) = callee {
                            assert_eq!(vm.heap.weak_value(handle), Some(function));
                        }
                        assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                        assert_eq!(vm.heap.root_count_for_test(), roots);
                        assert_eq!(vm.active_call_roots.len(), calls);
                        for (handle, value) in handles.iter().zip(&values) {
                            assert_eq!(
                                vm.heap.weak_value(*handle),
                                Some(*value),
                                "{kind}/{phase}/{abrupt}"
                            );
                        }
                        match outcome {
                            Ok(value) => {
                                let owner = vm.heap.root(value);
                                vm.collect_now(&program);
                                assert_eq!(
                                    vm.to_string(&program, value).unwrap(),
                                    match kind {
                                        "concat" => "abcXY",
                                        "anchor" => "<a name=\"X\">abc</a>",
                                        "substring" => "b",
                                        _ => unreachable!(),
                                    }
                                );
                                vm.heap.release_root(owner);
                            }
                            Err(error) => {
                                let atom = vm.intern_atom("kind");
                                let value = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                            }
                        }
                        if kind == "substring" {
                            for owner in owners {
                                vm.heap.release_root(owner);
                            }
                        }
                        vm.collect_now(&program);
                        for handle in handles {
                            assert!(vm.heap.weak_value(handle).is_none());
                        }
                        if let Some((_, handle)) = callee {
                            assert!(vm.heap.weak_value(handle).is_none());
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn string_raw_roots_fresh_raw_views_and_original_substitutions() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, phases) in [
            (
                "object",
                &[
                    "raw",
                    "length",
                    "length-number",
                    "segment0",
                    "segment0-string",
                    "substitution0-string",
                    "segment1",
                    "segment1-string",
                    "substitution1-string",
                    "segment2",
                    "segment2-string",
                ][..],
            ),
            (
                "string",
                &["raw", "substitution0-string", "substitution1-string"][..],
            ),
        ] {
            for phase in phases {
                for abrupt in [false, true] {
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function text(name,value){{return {{[Symbol.toPrimitive](hint){{hit(name);if(hint!=='string')throw {{kind:'hint'}};return value;}}}};}}
                    function template(){{return {{get raw(){{hit('raw');if('{kind}'==='string')return 'abc';return {{get length(){{hit('length');return {{[Symbol.toPrimitive](hint){{hit('length-number');return 3.9;}}}};}},get 0(){{hit('segment0');return text('segment0-string','a');}},get 1(){{hit('segment1');return text('segment1-string','b');}},get 2(){{hit('segment2');return text('segment2-string','c');}}}};}}}};}}
                    function first(){{return text('substitution0-string','X');}}
                    function second(){{return text('substitution1-string','Y');}}
                "#
                    );
                    let program = compile(&source, "string-raw-roots.js").unwrap();
                    vm.execute(&program).unwrap();
                    let mut values = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["template", "first", "second"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        values.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    if kind != "object" {
                        for owner in &owners {
                            vm.heap.release_root(*owner);
                        }
                    }
                    let handles = values
                        .iter()
                        .map(|value| vm.heap.weak_handle(*value).unwrap())
                        .collect::<Vec<_>>();
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let outcome = vm.string_raw(&program, &values);
                    assert_eq!(outcome.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    for (handle, value) in handles.iter().zip(&values) {
                        assert_eq!(
                            vm.heap.weak_value(*handle),
                            Some(*value),
                            "{kind}/{phase}/{abrupt}"
                        );
                    }
                    match outcome {
                        Ok(value) => {
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            assert_eq!(vm.to_string(&program, value).unwrap(), "aXbYc");
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), *phase);
                        }
                    }
                    if kind == "object" {
                        for owner in owners {
                            vm.heap.release_root(owner);
                        }
                    }
                    vm.collect_now(&program);
                    for handle in handles {
                        assert!(vm.heap.weak_value(handle).is_none());
                    }
                }
            }
        }
    }
}

#[test]
fn collator_construction_roots_boxed_locale_and_option_views() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for phase in [
            "usage-string",
            "usage",
            "length",
            "index",
            "locale",
            "localeMatcher",
            "collation",
            "numeric",
            "caseFirst",
            "sensitivity",
            "ignorePunctuation",
        ] {
            for abrupt in [false, true] {
                let source = format!(
                    r#"
                var trace=[];function traceLog(){{return trace.join(',');}}
                function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                function localeValue(){{return {{toString(){{hit('locale');return 'en';}}}};}}
                function usageValue(){{return {{toString(){{hit('usage-string');return 'sort';}}}};}}
                Object.defineProperty(Number.prototype,'length',{{configurable:true,get(){{hit('length');return 1;}}}});
                Object.defineProperty(Number.prototype,'0',{{configurable:true,get(){{hit('index');return localeValue();}}}});
                for(var key of ['usage','localeMatcher','collation','numeric','caseFirst','sensitivity','ignorePunctuation']){{
                    ((name)=>Object.defineProperty(Number.prototype,name,{{configurable:true,get(){{hit(name);if(name==='usage')return usageValue();return undefined;}}}}))(key);
                }}
                "#
                );
                let program = compile(&source, "collator-boxed-roots.js").unwrap();
                let mut vm = Vm::new(TestHost::default().with_test262());
                vm.execute(&program).unwrap();
                let constructor = *vm
                    .realm
                    .intrinsics
                    .intl_collator_constructors
                    .get(&vm.realm.globals)
                    .unwrap();
                let roots = vm.heap.root_count_for_test();
                let calls = vm.active_call_roots.len();
                let locales = if matches!(phase, "length" | "index" | "locale") {
                    Value::number(42.0)
                } else {
                    vm.heap.alloc_string("en".into())
                };
                let result = vm.intl_collator_construct(
                    &program,
                    &[locales, Value::number(7.0)],
                    constructor,
                );
                assert_eq!(result.is_ok(), !abrupt, "{phase}/{abrupt}");
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                match result {
                    Ok(collator) => {
                        let owner = vm.heap.root(collator);
                        vm.collect_now(&program);
                        let left = vm.heap.alloc_string("a".into());
                        let right = vm.heap.alloc_string("b".into());
                        let result = vm
                            .intl_collator_native(
                                &program,
                                Native::IntlCollatorCompare,
                                collator,
                                &[left, right],
                            )
                            .unwrap();
                        assert_eq!(result.as_number(), Some(-1.0), "{phase}");
                        let atom = vm.intern_atom("traceLog");
                        let logger = vm.own_property(vm.realm.globals, atom).unwrap();
                        let trace = vm
                            .call_value(&program, logger, Value::UNDEFINED, &[])
                            .unwrap();
                        let prefix = if matches!(phase, "length" | "index" | "locale") {
                            "length,index,locale,"
                        } else {
                            ""
                        };
                        assert_eq!(
                            vm.to_string(&program, trace).unwrap(),
                            format!(
                                "{prefix}usage,usage-string,localeMatcher,collation,numeric,caseFirst,sensitivity,ignorePunctuation"
                            ),
                            "{phase}"
                        );
                        vm.heap.release_root(owner);
                    }
                    Err(error) => {
                        let kind = vm.intern_atom("kind");
                        let value = vm
                            .own_property(error.thrown_value().unwrap(), kind)
                            .unwrap();
                        assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                    }
                }
                vm.collect_now(&program);
            }
        }
    }
}

#[test]
fn intl_coerced_option_views_survive_fresh_conversion_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, key, expected) in [
            ("NumberFormat", "style", "percent"),
            ("PluralRules", "type", "ordinal"),
            ("RelativeTimeFormat", "numeric", "auto"),
        ] {
            for phase in ["convert", "matcher", "selected"] {
                for abrupt in [false, true] {
                    let source = format!(
                        r#"
                    var trace=[];
                    function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function matcher(){{return {{toString(){{hit('convert');return 'lookup';}}}};}}
                    Object.defineProperty(Number.prototype,'localeMatcher',{{get(){{hit('matcher');return matcher();}},configurable:true}});
                    Object.defineProperty(Number.prototype,'{key}',{{get(){{hit('selected');return '{expected}';}},configurable:true}});
                    function inspect(value){{return Intl.{kind}.prototype.resolvedOptions.call(value)['{key}'];}}
                    function traceLog(){{return trace.join(',');}}
                    "#
                    );
                    let program = compile(&source, "intl-coerced-view.js").unwrap();
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    vm.execute(&program).unwrap();
                    let intl = vm.intern_atom("Intl");
                    let intl = vm.own_property(vm.realm.globals, intl).unwrap();
                    let name = vm.intern_atom(kind);
                    let constructor = vm.own_property(intl, name).unwrap();
                    let locale = vm.heap.alloc_string("en".into());
                    let weak = vm.heap.weak_handle(locale).unwrap();
                    let locale_owner = vm.heap.root(locale);
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = match kind {
                        "NumberFormat" => vm.intl_number_format_construct(
                            &program,
                            &[locale, Value::number(7.0)],
                            constructor,
                        ),
                        "PluralRules" => vm.intl_plural_rules_construct(
                            &program,
                            &[locale, Value::number(7.0)],
                            constructor,
                        ),
                        "RelativeTimeFormat" => vm.intl_relative_time_format_construct(
                            &program,
                            &[locale, Value::number(7.0)],
                            constructor,
                        ),
                        _ => unreachable!(),
                    };
                    assert_eq!(result.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    assert_eq!(vm.heap.weak_value(weak), Some(locale), "{kind}/{phase}");
                    match result {
                        Ok(instance) => {
                            let owner = vm.heap.root(instance);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("inspect");
                            let inspect = vm.own_property(vm.realm.globals, atom).unwrap();
                            let output = vm
                                .call_value(&program, inspect, Value::UNDEFINED, &[instance])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, output).unwrap(),
                                expected,
                                "{kind}/{phase}"
                            );
                            let atom = vm.intern_atom("traceLog");
                            let logger = vm.own_property(vm.realm.globals, atom).unwrap();
                            let trace = vm
                                .call_value(&program, logger, Value::UNDEFINED, &[])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, trace).unwrap(),
                                "matcher,convert,selected",
                                "{kind}/{phase}"
                            );
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    vm.heap.release_root(locale_owner);
                    vm.collect_now(&program);
                    assert!(vm.heap.weak_value(weak).is_none());
                }
            }
        }
    }
}

#[test]
fn intl_constructor_views_survive_prototype_and_option_callbacks() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for (kind, key, expected) in [
            ("DisplayNames", "type", "language"),
            ("DurationFormat", "style", "short"),
            ("DateTimeFormat", "year", "numeric"),
        ] {
            for phase in ["prototype", "locale", "matcher", "convert", "selected"] {
                for abrupt in [false, true] {
                    let source = format!(
                        r#"
                    function hit(name){{if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function matcher(){{return {{toString(){{hit('convert');return 'lookup';}}}};}}
                    function locales(){{return {{get length(){{hit('locale');return 1;}},0:'en'}};}}
                    function options(){{var value={{get localeMatcher(){{hit('matcher');return matcher();}}}};Object.defineProperty(value,'{key}',{{get(){{hit('selected');return '{expected}';}}}});return value;}}
                    function target(){{return new Proxy(function(){{}},{{get(value,key){{if(key==='prototype'){{hit('prototype');return {{marker:'expected'}};}}return Reflect.get(value,key);}}}});}}
                    function inspect(value){{return Object.getPrototypeOf(value).marker+':'+Intl.{kind}.prototype.resolvedOptions.call(value)['{key}'];}}
                    "#
                    );
                    let program = compile(&source, "intl-constructor-views.js").unwrap();
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    vm.execute(&program).unwrap();
                    let mut args = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["locales", "options", "target"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        args.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = match kind {
                        "DisplayNames" => {
                            vm.intl_display_names_construct(&program, &args[..2], args[2])
                        }
                        "DurationFormat" => {
                            vm.intl_duration_format_construct(&program, &args[..2], args[2])
                        }
                        "DateTimeFormat" => {
                            vm.intl_date_time_format_construct(&program, &args[..2], args[2])
                        }
                        _ => unreachable!(),
                    };
                    assert_eq!(result.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(instance) => {
                            let owner = vm.heap.root(instance);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("inspect");
                            let inspect = vm.own_property(vm.realm.globals, atom).unwrap();
                            let output = vm
                                .call_value(&program, inspect, Value::UNDEFINED, &[instance])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, output).unwrap(),
                                format!("expected:{expected}"),
                                "{kind}/{phase}"
                            );
                            vm.heap.release_root(owner);
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    vm.collect_now(&program);
                }
            }
        }
    }
}

#[test]
fn intl_locale_roots_boxed_options_and_fresh_prototype_views() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in ["boxed", "object"] {
            for phase in [
                "language-string",
                "prototype",
                "tag",
                "language",
                "region",
                "calendar",
                "calendar-string",
            ] {
                for abrupt in [false, true] {
                    let source = format!(
                        r#"
                    var trace=[];
                    function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function language(){{return {{toString(){{hit('language-string');return 'fr';}}}};}}
                    function calendar(){{return {{toString(){{hit('calendar-string');return 'gregory';}}}};}}
                    Object.defineProperty(Number.prototype,'language',{{configurable:true,get(){{hit('language');return language();}}}});
                    Object.defineProperty(Number.prototype,'region',{{configurable:true,get(){{hit('region');return 'US';}}}});
                    Object.defineProperty(Number.prototype,'calendar',{{configurable:true,get(){{hit('calendar');return calendar();}}}});
                    function tag(){{return {{toString(){{hit('tag');return 'en';}}}};}}
                    function options(){{return '{kind}'==='boxed'?7:Object.create(Number.prototype);}}
                    function target(){{return new Proxy(function(){{}},{{get(value,key){{if(key==='prototype'){{hit('prototype');return {{marker:'expected'}};}}return Reflect.get(value,key);}}}});}}
                    function inspect(value){{return Object.getPrototypeOf(value).marker+'|'+Intl.Locale.prototype.toString.call(value);}}
                    function traceLog(){{return trace.join(',');}}
                    "#
                    );
                    let program = compile(&source, "intl-locale-views.js").unwrap();
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    vm.execute(&program).unwrap();
                    let mut args = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["tag", "options", "target"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        args.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let result = vm.intl_namespace_construct(
                        &program,
                        Native::IntlLocale,
                        &args[..2],
                        args[2],
                    );
                    assert_eq!(result.is_ok(), !abrupt, "{kind}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(instance) => {
                            let weak = vm.heap.weak_handle(instance).unwrap();
                            let owner = vm.heap.root(instance);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("inspect");
                            let function = vm.own_property(vm.realm.globals, atom).unwrap();
                            let output = vm
                                .call_value(&program, function, Value::UNDEFINED, &[instance])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, output).unwrap(),
                                "expected|fr-US-u-ca-gregory",
                                "{kind}/{phase}"
                            );
                            let atom = vm.intern_atom("traceLog");
                            let function = vm.own_property(vm.realm.globals, atom).unwrap();
                            let output = vm
                                .call_value(&program, function, Value::UNDEFINED, &[])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, output).unwrap(),
                                "prototype,tag,language,language-string,region,calendar,calendar-string",
                                "{kind}/{phase}"
                            );
                            vm.heap.release_root(owner);
                            vm.collect_now(&program);
                            assert!(vm.heap.weak_value(weak).is_none());
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    vm.collect_now(&program);
                }
            }
        }
    }
}

#[test]
fn intl_supported_locales_roots_callback_views_and_releases_results() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for service in [
            "RelativeTimeFormat",
            "Segmenter",
            "Collator",
            "NumberFormat",
            "DateTimeFormat",
            "PluralRules",
            "DurationFormat",
            "ListFormat",
            "DisplayNames",
        ] {
            for kind in ["boxed", "object"] {
                for phase in [
                    "length",
                    "has",
                    "index",
                    "locale",
                    "matcher",
                    "matcher-string",
                ] {
                    for abrupt in [false, true] {
                        let source = format!(
                            r#"
                        var trace=[];
                        function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                        function locale(){{return {{toString(){{hit('locale');return 'en';}}}};}}
                        function matcher(){{return {{toString(){{hit('matcher-string');return 'lookup';}}}};}}
                        Object.defineProperty(Number.prototype,'localeMatcher',{{configurable:true,get(){{hit('matcher');return matcher();}}}});
                        function locales(){{return new Proxy({{length:1}},{{get(value,key){{if(key==='length'){{hit('length');return 1;}}hit('index');return locale();}},has(){{hit('has');return true;}}}});}}
                        function options(){{return '{kind}'==='boxed'?7:Object.create(Number.prototype);}}
                        function query(){{return Intl['{service}'].supportedLocalesOf;}}
                        function inspect(value){{return value.join(',')+'|'+trace.join(',');}}
                        "#
                        );
                        let program = compile(&source, "intl-supported-locales-views.js").unwrap();
                        let mut vm = Vm::new(TestHost::default().with_test262());
                        vm.execute(&program).unwrap();
                        let mut inputs = Vec::new();
                        let mut owners = Vec::new();
                        for name in ["query", "locales", "options"] {
                            let atom = vm.intern_atom(name);
                            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                            let value = vm
                                .call_value(&program, factory, Value::UNDEFINED, &[])
                                .unwrap();
                            inputs.push(value);
                            owners.push(vm.heap.root(value));
                        }
                        let roots = vm.heap.root_count_for_test();
                        let calls = vm.active_call_roots.len();
                        let result =
                            vm.call_value(&program, inputs[0], Value::UNDEFINED, &inputs[1..]);
                        assert_eq!(result.is_ok(), !abrupt, "{service}/{kind}/{phase}/{abrupt}");
                        assert_eq!(vm.heap.root_count_for_test(), roots);
                        assert_eq!(vm.active_call_roots.len(), calls);
                        match result {
                            Ok(value) => {
                                let weak = vm.heap.weak_handle(value).unwrap();
                                let owner = vm.heap.root(value);
                                vm.collect_now(&program);
                                let atom = vm.intern_atom("inspect");
                                let function = vm.own_property(vm.realm.globals, atom).unwrap();
                                let output = vm
                                    .call_value(&program, function, Value::UNDEFINED, &[value])
                                    .unwrap();
                                assert_eq!(
                                    vm.to_string(&program, output).unwrap(),
                                    "en|length,has,index,locale,matcher,matcher-string",
                                    "{service}/{kind}/{phase}"
                                );
                                vm.heap.release_root(owner);
                                vm.collect_now(&program);
                                assert!(vm.heap.weak_value(weak).is_none());
                            }
                            Err(error) => {
                                let atom = vm.intern_atom("kind");
                                let value = vm
                                    .own_property(error.thrown_value().unwrap(), atom)
                                    .unwrap();
                                assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                            }
                        }
                        for owner in owners {
                            vm.heap.release_root(owner);
                        }
                        vm.collect_now(&program);
                    }
                }
            }
        }
    }
}

#[test]
fn list_format_roots_fresh_iterators_across_step_getters() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for method in ["format", "formatToParts"] {
            for phase in ["done", "value", "iterator", "factory", "get-next", "next"] {
                for abrupt in [false, true] {
                    let source = format!(
                        r#"
                    var trace=[];
                    function hit(name){{trace.push(name);if(name==='{phase}'){{$262.gc();if({abrupt})throw {{kind:name}};}}}}
                    function step(index){{return {{get done(){{hit('done');return index===2;}},get value(){{hit('value');return index===0?'A':'B';}}}};}}
                    function next(){{hit('next');return step(this.index++);}}
                    function source(){{return {{get [Symbol.iterator](){{hit('iterator');return function(){{hit('factory');return {{index:0,get next(){{hit('get-next');return next;}},return(){{throw 'closed step failure';}}}};}};}}}};}}
                    function formatter(){{return new Intl.ListFormat('en');}}
                    function invoke(formatter,source){{return formatter['{method}'](source);}}
                    function inspect(value){{return (typeof value==='string'?value:value.map(part=>part.value).join(''))+'|'+trace.join(',');}}
                    "#
                    );
                    let program = compile(&source, "list-format-iterator-views.js").unwrap();
                    let mut vm = Vm::new(TestHost::default().with_test262());
                    vm.execute(&program).unwrap();
                    let mut inputs = Vec::new();
                    let mut owners = Vec::new();
                    for name in ["formatter", "source"] {
                        let atom = vm.intern_atom(name);
                        let factory = vm.own_property(vm.realm.globals, atom).unwrap();
                        let value = vm
                            .call_value(&program, factory, Value::UNDEFINED, &[])
                            .unwrap();
                        inputs.push(value);
                        owners.push(vm.heap.root(value));
                    }
                    let roots = vm.heap.root_count_for_test();
                    let calls = vm.active_call_roots.len();
                    let atom = vm.intern_atom("invoke");
                    let function = vm.own_property(vm.realm.globals, atom).unwrap();
                    let result = vm.call_value(&program, function, Value::UNDEFINED, &inputs);
                    assert_eq!(result.is_ok(), !abrupt, "{method}/{phase}/{abrupt}");
                    assert_eq!(vm.heap.root_count_for_test(), roots);
                    assert_eq!(vm.active_call_roots.len(), calls);
                    match result {
                        Ok(value) => {
                            let weak = vm.heap.weak_handle(value).unwrap();
                            let owner = vm.heap.root(value);
                            vm.collect_now(&program);
                            let atom = vm.intern_atom("inspect");
                            let function = vm.own_property(vm.realm.globals, atom).unwrap();
                            let output = vm
                                .call_value(&program, function, Value::UNDEFINED, &[value])
                                .unwrap();
                            assert_eq!(
                                vm.to_string(&program, output).unwrap(),
                                "A and B|iterator,factory,get-next,next,done,value,next,done,value,next,done",
                                "{method}/{phase}"
                            );
                            vm.heap.release_root(owner);
                            vm.collect_now(&program);
                            assert!(vm.heap.weak_value(weak).is_none());
                        }
                        Err(error) => {
                            let atom = vm.intern_atom("kind");
                            let value = vm
                                .own_property(error.thrown_value().unwrap(), atom)
                                .unwrap();
                            assert_eq!(vm.to_string(&program, value).unwrap(), phase);
                        }
                    }
                    for owner in owners {
                        vm.heap.release_root(owner);
                    }
                    vm.collect_now(&program);
                }
            }
        }
    }
}

#[test]
fn segmenter_cursors_own_source_and_release_derived_records() {
    let cases: &[(&str, &str, &[(&str, usize)])] = &[
        ("grapheme", "A😀B", &[("A", 0), ("😀", 1), ("B", 3)]),
        ("word", "hi there", &[("hi", 0), (" ", 2), ("there", 3)]),
        ("sentence", "Hi. Bye.", &[("Hi. ", 0), ("Bye.", 4)]),
    ];
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for &(granularity, input, expected) in cases {
            let source = format!(
                r#"
            function formatter(){{return new Intl.Segmenter('en',{{granularity:'{granularity}'}});}}
            function input(){{return {{toString(){{$262.gc();return '{input}';}}}};}}
            function make(formatter){{return formatter.segment(input());}}
            "#
            );
            let program = compile(&source, "segmenter-source-ownership.js").unwrap();
            let mut vm = Vm::new(TestHost::default().with_test262());
            vm.execute(&program).unwrap();
            let initial = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let atom = vm.intern_atom("formatter");
            let function = vm.own_property(vm.realm.globals, atom).unwrap();
            let formatter = vm
                .call_value(&program, function, Value::UNDEFINED, &[])
                .unwrap();
            let formatter_weak = vm.heap.weak_handle(formatter).unwrap();
            let formatter_owner = vm.heap.root(formatter);
            let atom = vm.intern_atom("make");
            let function = vm.own_property(vm.realm.globals, atom).unwrap();
            let segments = vm
                .call_value(&program, function, Value::UNDEFINED, &[formatter])
                .unwrap();
            let segments_weak = vm.heap.weak_handle(segments).unwrap();
            let segments_owner = vm.heap.root(segments);
            let iterator = vm.get_iterator(&program, segments).unwrap();
            let iterator_weak = vm.heap.weak_handle(iterator).unwrap();
            let iterator_owner = vm.heap.root(iterator);
            vm.heap.release_root(formatter_owner);
            vm.heap.release_root(segments_owner);
            vm.collect_now(&program);
            assert!(
                vm.heap.weak_value(formatter_weak).is_some(),
                "{granularity}: cursor must own formatter"
            );
            assert!(
                vm.heap.weak_value(segments_weak).is_none(),
                "{granularity}: cursor must release public Segments wrapper"
            );
            let mut input_identity = None;
            for &(text, index) in expected {
                let roots = vm.heap.root_count_for_test();
                let record = vm.iterator_step_value(&program, iterator).unwrap().unwrap();
                assert_eq!(vm.heap.root_count_for_test(), roots);
                assert_eq!(vm.active_call_roots.len(), calls);
                let weak = vm.heap.weak_handle(record).unwrap();
                let owner = vm.heap.root(record);
                vm.collect_now(&program);
                let atom = vm.intern_atom("segment");
                let value = vm.own_property(record, atom).unwrap();
                assert_eq!(vm.to_string(&program, value).unwrap(), text);
                let atom = vm.intern_atom("index");
                assert_eq!(
                    vm.own_property(record, atom).unwrap(),
                    Value::number(index as f64)
                );
                let atom = vm.intern_atom("input");
                let value = vm.own_property(record, atom).unwrap();
                assert_eq!(vm.to_string(&program, value).unwrap(), input);
                if let Some(previous) = input_identity {
                    assert_eq!(previous, value);
                } else {
                    input_identity = Some(value);
                }
                vm.heap.release_root(owner);
                vm.collect_now(&program);
                assert!(
                    vm.heap.weak_value(weak).is_none(),
                    "derived record must not be retained by its source"
                );
            }
            assert!(
                vm.iterator_step_value(&program, iterator)
                    .unwrap()
                    .is_none()
            );
            assert!(
                vm.iterator_step_value(&program, iterator)
                    .unwrap()
                    .is_none()
            );
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(formatter_weak).is_some());
            assert!(vm.heap.weak_value(segments_weak).is_none());
            vm.heap.release_root(iterator_owner);
            vm.collect_now(&program);
            for weak in [formatter_weak, segments_weak, iterator_weak] {
                assert!(vm.heap.weak_value(weak).is_none());
            }
            assert_eq!(vm.heap.root_count_for_test(), initial);
            assert_eq!(vm.active_call_roots.len(), calls);
        }
    }
}

#[test]
fn number_format_legacy_views_release_after_bound_or_options_completion() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for kind in ["getter", "options"] {
            let source = format!(
                r#"
            var base=Object.create(Intl.NumberFormat.prototype);Intl.NumberFormat.call(base);
            var symbol=Object.getOwnPropertySymbols(base)[0];var last;
            function fresh(){{var value=new Intl.NumberFormat('en',{{style:'percent'}});Object.defineProperty(value,symbol,{{get(){{throw 'read actual fallback';}}}});return value;}}
            function wrapper(){{var object=Object.create(Intl.NumberFormat.prototype);Object.defineProperty(object,symbol,{{get(){{$262.gc();last=fresh();return last;}}}});return object;}}
            function method(){{return '{kind}'==='getter'?Object.getOwnPropertyDescriptor(Intl.NumberFormat.prototype,'format').get:Intl.NumberFormat.prototype.resolvedOptions;}}
            function input(){{return {{valueOf(){{$262.gc();return 1;}}}};}}
            "#
            );
            let program = compile(&source, "number-format-legacy-views.js").unwrap();
            let mut vm = Vm::new(TestHost::default().with_test262());
            vm.execute(&program).unwrap();
            let atom = vm.intern_atom("wrapper");
            let function = vm.own_property(vm.realm.globals, atom).unwrap();
            let wrapper = vm
                .call_value(&program, function, Value::UNDEFINED, &[])
                .unwrap();
            let wrapper_owner = vm.heap.root(wrapper);
            let atom = vm.intern_atom("method");
            let factory = vm.own_property(vm.realm.globals, atom).unwrap();
            let method = vm
                .call_value(&program, factory, Value::UNDEFINED, &[])
                .unwrap();
            let roots = vm.heap.root_count_for_test();
            let calls = vm.active_call_roots.len();
            let result = vm.call_value(&program, method, wrapper, &[]).unwrap();
            assert_eq!(vm.heap.root_count_for_test(), roots);
            assert_eq!(vm.active_call_roots.len(), calls);
            let result_weak = vm.heap.weak_handle(result).unwrap();
            let owner = vm.heap.root(result);
            let atom = vm.intern_atom("last");
            let formatter = vm.own_property(vm.realm.globals, atom).unwrap();
            let weak = vm.heap.weak_handle(formatter).unwrap();
            vm.set_property(vm.realm.globals, atom, Value::UNDEFINED)
                .unwrap();
            vm.collect_now(&program);
            assert_eq!(vm.heap.weak_value(weak).is_some(), kind == "getter");
            if kind == "getter" {
                let atom = vm.intern_atom("input");
                let function = vm.own_property(vm.realm.globals, atom).unwrap();
                let input = vm
                    .call_value(&program, function, Value::UNDEFINED, &[])
                    .unwrap();
                let output = vm
                    .call_value(&program, result, Value::UNDEFINED, &[input])
                    .unwrap();
                assert_eq!(vm.to_string(&program, output).unwrap(), "100%");
            } else {
                let atom = vm.intern_atom("style");
                let value = vm.own_property(result, atom).unwrap();
                assert_eq!(vm.to_string(&program, value).unwrap(), "percent");
            }
            vm.heap.release_root(owner);
            vm.heap.release_root(wrapper_owner);
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(weak).is_none());
            assert!(vm.heap.weak_value(result_weak).is_none());
            assert_eq!(vm.active_call_roots.len(), calls);
            assert_eq!(vm.heap.root_count_for_test(), roots - 1);
        }
    }
}

#[test]
fn weak_ref_targets_are_kept_until_host_checkpoint() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        for dereference in [false, true] {
            let mut vm = Vm::new(TestHost::default());
            let program = compile("var weak = new WeakRef({});", "weak-ref-job.js").unwrap();
            vm.execute(&program).unwrap();
            let key = vm.intern_atom("weak");
            let weak = vm.own_property(vm.realm.globals, key).unwrap();
            let target = match vm.heap.get(weak) {
                Some(super::Cell::WeakRef { target, .. }) => target.unwrap(),
                _ => panic!("expected weak reference"),
            };
            if dereference {
                vm.clear_kept_objects();
                let value = vm
                    .call_collection_native(&program, Native::WeakRefDeref, weak, &[])
                    .unwrap();
                assert!(!value.is_undefined());
            }
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(target).is_some());
            vm.clear_kept_objects();
            vm.collect_now(&program);
            assert!(vm.heap.weak_value(target).is_none());
        }
    }
}

#[test]
fn host_byte_views_preserve_intrinsics_backing_and_gc_edges() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let mut vm = Vm::new(TestHost::default());
        let program = compile("0;", "host-bytes.js").unwrap();
        vm.execute(&program).unwrap();
        for bytes in [vec![], vec![0, 127, 128, 255]] {
            let view = vm.embedding_uint8_array(&bytes).unwrap();
            let root = vm.heap.root(view);
            let prototype = match vm.heap.get(view) {
                Some(super::Cell::TypedArray {
                    object,
                    kind,
                    buffer,
                    ..
                }) => {
                    assert_eq!(*kind, TypedArrayKind::Uint8);
                    assert!(matches!(
                        vm.heap.get(*buffer),
                        Some(super::Cell::ArrayBuffer {
                            shared: false,
                            detached: false,
                            resizable: false,
                            immutable: false,
                            ..
                        })
                    ));
                    object.proto()
                }
                _ => panic!("expected Uint8Array"),
            };
            assert_eq!(prototype, vm.typed_array_proto(TypedArrayKind::Uint8));
            vm.collect_now(&program);
            let view = vm.heap.root_value(root).unwrap();
            assert_eq!(vm.embedding_view_bytes(view).unwrap(), bytes);
            vm.heap.release_root(root);
        }
    }
}

#[test]
fn private_host_weak_roots_clear_before_slot_reuse_without_kept_objects() {
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let mut vm = Vm::new(TestHost::default());
        let program = compile("0;", "host-weak.js").unwrap();
        vm.execute(&program).unwrap();
        let target = vm
            .heap
            .alloc(super::Cell::Object(Vm::<TestHost>::empty_object(
                Value::NULL,
            )));
        let strong = vm.heap.root(target);
        let weak = vm.embedding_host_weak_root(strong).unwrap();
        vm.collect_now(&program);
        assert_eq!(
            vm.embedding_host_weak_value(weak),
            Some(vm.heap.root_value(strong).unwrap())
        );
        vm.heap.release_root(strong);
        vm.collect_now(&program);
        assert!(vm.embedding_host_weak_value(weak).is_none());
        let replacement = vm
            .heap
            .alloc(super::Cell::Object(Vm::<TestHost>::empty_object(
                Value::NULL,
            )));
        assert!(vm.heap.is_live(replacement));
        assert!(vm.embedding_host_weak_value(weak).is_none());
        let other = Vm::new(TestHost::default());
        assert!(other.embedding_host_weak_value(weak).is_none());
        vm.heap.release_root(weak);
    }
}

#[test]
fn string_repeat_limits_preserve_lazy_utf16_length_and_fractional_counts() {
    let limit = super::wtf16::MAX_STRING_UNITS;
    let source = format!(
        r#"
      print('x'.repeat({limit}).length);
      print('x'.repeat(70000000).repeat(2).length);
      print('xy'.repeat(-0.5).length);
      print('日'.repeat(3).length);
      try {{ 'x'.repeat({limit} + 1); }}
      catch (error) {{ print(error.name + ':' + error.message); }}
    "#
    );
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let program = compile(&source, "string-repeat-limit.js").unwrap();
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(&program).unwrap();
        assert_eq!(
            output.borrow().as_slice(),
            [
                limit.to_string(),
                "140000000".into(),
                "0".into(),
                "3".into(),
                "RangeError:Invalid string length".into()
            ]
        );
    }
}

#[test]
fn empty_string_repeat_keeps_coercion_order_without_allocating_a_copy() {
    let source = r#"
      let conversions = 0;
      const empty = '';
      const repeated = empty.repeat({
        valueOf() { conversions++; return Number.MAX_SAFE_INTEGER; }
      });
      print(repeated === '');
      print(conversions);
      print(new String('').repeat(Number.MAX_SAFE_INTEGER) === '');
      for (const invalid of [Infinity, -1]) {
        try { empty.repeat(invalid); print('missing error'); }
        catch (error) { print(error.name); }
      }
      try { empty.repeat(Symbol()); print('missing error'); }
      catch (error) { print(error.name); }
    "#;
    let program = Engine::specialize(source, "empty-repeat-fast-path.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["true", "1", "true", "RangeError", "RangeError", "TypeError"]
    );
}

#[test]
fn decode_uri_reuses_unescaped_input_and_preserves_uri_semantics() {
    let source = r#"
      const input = 'plain café / text';
      print(decodeURI(input) === input);
      print(decodeURI('%E2%82%AC%3F') + '|' + decodeURIComponent('%E2%82%AC%3F'));
      for (const malformed of ['%', '%2', '%C0%AF', '%E0%80%AF', '%ED%A0%80']) {
        try { decodeURI(malformed); print('missing error'); }
        catch (error) { print(error.name + ':' + error.message); }
      }
    "#;
    let program = Engine::specialize(source, "decode-uri-fast-path.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        [
            "true",
            "€%3F|€?",
            "URIError:URI malformed",
            "URIError:URI malformed",
            "URIError:URI malformed",
            "URIError:URI malformed",
            "URIError:URI malformed",
        ]
    );
}

#[test]
fn string_constructor_and_template_coercion_preserve_wtf16_and_original_hooks() {
    let source = r#"
      for (const text of ["\ud800", "\ud801", "\udfff", "a\ud800b", "\ud83d\ude00"]) {
        const box = new String(text);
        const log = [];
        const object = { [Symbol.toPrimitive](hint) { log.push(hint); return text; } };
        for (const actual of [String(text), box.valueOf(), `${text}`, String(object), `${object}`]) {
          if (actual !== text) throw Error('UTF-16 coercion changed original units');
        }
        if (log.join(',') !== 'string,string') throw Error('coercion hint or count');
      }
      if (String(Symbol('x')) !== 'Symbol(x)') throw Error('explicit Symbol conversion');
      for (const convert of [value => `${value}`, value => new String(value)]) {
        let caught = false;
        try { convert(Symbol('x')); } catch (error) { caught = error instanceof TypeError; }
        if (!caught) throw Error('implicit Symbol conversion accepted');
      }
    "#;
    for program in [
        Engine::compile(crate::ExecutionRequest { source, name: "wtf16-string-coercion.js", kind: crate::SourceKind::Script }).unwrap(),
        Engine::specialize(source, "wtf16-string-coercion.js").unwrap(),
    ] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn script_directive_completion_survives_declarations_and_preserves_string_units() {
    for program in [
        Engine::compile(crate::ExecutionRequest { source: "", name: "directive-completion.js", kind: crate::SourceKind::Script }).unwrap(),
        Engine::specialize("", "directive-completion.js").unwrap(),
    ] {
        for (source, expected) in [
            ("'first'; const x=1;", Some("first")),
            ("'use strict'; let x;", Some("use strict")),
            ("'a'; 'b'; var x;", Some("b")),
            ("'first'; function f(){}", Some("first")),
            ("'first'; if(false){2}", None),
            ("'first'; ; debugger;", Some("first")),
        ] {
            let mut vm = Vm::new(TestHost::default());
            vm.execute(&program).unwrap();
            let value = vm.eval_global_script_named(&program, source, false, "directive-completion.js").unwrap();
            match expected {
                Some(expected) => assert!(vm.heap.string(value).is_some_and(|text| text.host_string() == expected), "{source}"),
                None => assert!(value.is_undefined(), "{source}"),
            }
        }
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&program).unwrap();
        let value = vm.eval_global_script_named(&program, r"'\ud800'; var x;", false, "directive-completion.js").unwrap();
        assert!(vm.heap.string(value).is_some_and(|text| text.units().wide().as_ref() == [0xd800]));
    }
}

#[test]
fn primitive_intrinsic_prototypes_survive_global_replacement_realms_and_collection() {
    let setup = r#"
      globalThis.savedPrimitives={constructors:[Number,Boolean,Symbol,BigInt,String],values:[17,true,Symbol('original'),19n,'text']};
      savedPrimitives.prototypes=savedPrimitives.constructors.map(constructor=>constructor.prototype);
      for(const prototype of savedPrimitives.prototypes)prototype.originalReceiver=function(){'use strict';return this;};
      for(const name of ['Number','Boolean','Symbol','BigInt','String'])globalThis[name]=function Replacement(){};
    "#;
    let check = r#"
      for(let i=0;i<savedPrimitives.values.length;i++){
        const value=savedPrimitives.values[i],prototype=savedPrimitives.prototypes[i];
        if(value.originalReceiver()!==value||value.valueOf()!==value)throw Error('primitive receiver or prototype changed');
        if(Object.getPrototypeOf(Object(value))!==prototype)throw Error('boxed intrinsic prototype changed');
      }
      if((19n).toString()!=='19'||(17).toString()!=='17'||true.toString()!=='true')throw Error('primitive formatting changed');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source:setup,name:"primitive-intrinsics.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(setup,"primitive-intrinsics.js").unwrap()] {
        let mut vm=Vm::new(TestHost::default());vm.execute(&program).unwrap();
        let original=vm.realm.globals;
        let other=vm.create_realm_global(&program).unwrap();vm.switch_realm_global(other);
        vm.eval_global_script_named(&program,setup,false,"foreign-primitive-intrinsics.js").unwrap();
        vm.collect_now(&program);vm.eval_global_script_named(&program,check,false,"foreign-primitive-check.js").unwrap();
        vm.switch_realm_global(original);vm.collect_now(&program);
        vm.eval_global_script_named(&program,check,false,"original-primitive-check.js").unwrap();
    }
}

#[test]
fn typed_array_getter_diagnostics_do_not_observe_guest_conversion_hooks() {
    let source=r#"
      let hooks=0;const object={toString(){hooks++;throw Error('guest hook');},[Symbol.toStringTag]:'Special'};
      for(const property of ['length','byteLength','byteOffset','buffer']) {
        const getter=Object.getOwnPropertyDescriptor(Object.getPrototypeOf(Uint8Array.prototype),property).get;
        const accessor={};Object.defineProperty(accessor,'toString',{get(){hooks++;throw Error('getter');}});
        const tagged={toString(){hooks++;throw Error('conversion');}};Object.defineProperty(tagged,Symbol.toStringTag,{get(){hooks++;throw Error('tag');}});
        for(const [value,label] of [[null,'null'],[undefined,'undefined'],[{},'#<Object>'],[object,'[object Special]'],[Object.create(Uint8Array.prototype),'[object Object]'],[Object.create(null),'[object Object]'],[accessor,'[object Object]'],[tagged,'[object Object]'],[new DataView(new ArrayBuffer(0)),'#<DataView>']]) {
          try { getter.call(value);throw Error('accepted invalid receiver'); }
          catch(error) { if(!(error instanceof TypeError)||error.message!==`Method get TypedArray.prototype.${property} called on incompatible receiver ${label}`)throw error; }
        }
      }
      if(hooks!==0)throw Error('diagnostics invoked guest conversion');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"typed-getter-diagnostics.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"typed-getter-diagnostics.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn nullish_property_diagnostics_include_original_property_names() {
    let source=r#"
      let hooks=0;const key={toString(){hooks++;throw Error('key hook');}};
      for(const value of [null,undefined]) {
        for(const property of ['length',1,Symbol('x')]) {
          try { value[property]; throw Error('accepted nullish receiver'); }
          catch(error) { if(!(error instanceof TypeError)||error.message!==`Cannot read properties of ${value} (reading '${String(property)}')`)throw error; }
        }
        try { value[key]; throw Error('accepted object key'); }
        catch(error) { if(!(error instanceof TypeError)||error.message!==`Cannot read properties of ${value}`)throw error; }
      }
      if(hooks!==0)throw Error('rejected base invoked key conversion');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest{source,name:"nullish-property-errors.js",kind:crate::SourceKind::Script}).unwrap(),Engine::specialize(source,"nullish-property-errors.js").unwrap()] {Vm::new(TestHost::default()).execute(&program).unwrap();}
}

#[test]
fn typed_array_offset_and_content_errors_preserve_numeric_hooks_and_targets() {
    let source = r#"
const ab=new ArrayBuffer(5),calls=[];
try{new Uint8Array(ab,{valueOf(){calls.push('offset');return -1.5}},{valueOf(){calls.push('length');return 1}})}catch(e){print(e.name);print(e.message)}
print(calls.join());print(new Uint8Array(ab,-0.5).length);
const marker={};try{new Uint8Array(ab,{valueOf(){throw marker}})}catch(e){print(e===marker)}
for(const [target,source] of [[new Uint8Array(2),new BigInt64Array(1)],[new BigInt64Array(2),new Uint8Array(1)]]){
 try{target.set(source)}catch(e){print(e.name);print(e.message)}
 print(Array.from(target,String).join());
}
"#;
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let output=Rc::new(RefCell::new(Vec::new()));
        let mut vm=Vm::new(TestHost::writing_to(output.clone()));
        let program=compile(source,"typed-array-offset-content-errors.js").unwrap();
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(),[
            "RangeError","Start offset -1.5 is outside the bounds of the buffer","offset","5","true",
            "TypeError","Cannot mix BigInt and other types, use explicit conversions","0,0",
            "TypeError","Cannot mix BigInt and other types, use explicit conversions","0,0",
        ]);
    }
}

#[test]
fn rejected_property_assignments_preserve_diagnostics_and_proxy_effects() {
    let source = r#"
const out=[],events=[],marker={};function store(target,key){'use strict';target[key]=1}function field(target){'use strict';target.x=1}function check(tag,fn){try{fn();out.push([tag,'ok'])}catch(e){out.push([tag,e.name,e.message,e===marker])}}
for(const [name,target] of [['undefined',undefined],['null',null],['boolean',false],['number',1],['string','abc'],['symbol',Symbol('s')],['bigint',1n],['frozen',Object.freeze({x:0})],['sealed',Object.preventExtensions({})],['getter',Object.defineProperty({},'x',{get(){events.push('unexpected-get')}})]])for(const fn of [field,t=>store(t,'x')])check(name,()=>fn(target));
const handler={get set(){events.push('trap-get');return function(t,k,v){events.push('trap-call');delete handler.set;return false}}};check('proxy',()=>store(new Proxy({},handler),'x'));
check('proxy-readonly',()=>store(new Proxy(Object.freeze({x:0}),{}),'x'));check('proxy-extend',()=>store(new Proxy(Object.preventExtensions({}),{}),'x'));check('nested',()=>field(new Proxy(new Proxy({}, {set(){events.push('inner');return false}}),{})));
check('setter-original',()=>field({set x(v){throw marker}}));const hostile={get toString(){events.push('unexpected-string');throw marker},get [Symbol.toStringTag](){events.push('unexpected-tag');throw marker}};Object.defineProperty(hostile,'x',{value:0,writable:false});check('hostile',()=>field(hostile));for(const key of ['0','length','5'])check(['string',key],()=>store('abc',key));Object.defineProperty(Boolean.prototype,'x',{configurable:true,get(){events.push('unexpected-boolean-get')}});check('primitive-getter',()=>field(false));delete Boolean.prototype.x;out.push(['reflect',Reflect.set(new Proxy({}, {set(){return false}}),'x',1)]);out.push(events);print(JSON.stringify(out));

"#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "assignment-diagnostics.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "assignment-diagnostics.js").unwrap()] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), [r#"[["undefined","TypeError","Cannot set properties of undefined (setting 'x')",false],["undefined","TypeError","Cannot set properties of undefined (setting 'x')",false],["null","TypeError","Cannot set properties of null (setting 'x')",false],["null","TypeError","Cannot set properties of null (setting 'x')",false],["boolean","TypeError","Cannot create property 'x' on boolean 'false'",false],["boolean","TypeError","Cannot create property 'x' on boolean 'false'",false],["number","TypeError","Cannot create property 'x' on number '1'",false],["number","TypeError","Cannot create property 'x' on number '1'",false],["string","TypeError","Cannot create property 'x' on string 'abc'",false],["string","TypeError","Cannot create property 'x' on string 'abc'",false],["symbol","TypeError","Cannot create property 'x' on symbol 'Symbol(s)'",false],["symbol","TypeError","Cannot create property 'x' on symbol 'Symbol(s)'",false],["bigint","TypeError","Cannot create property 'x' on bigint '1'",false],["bigint","TypeError","Cannot create property 'x' on bigint '1'",false],["frozen","TypeError","Cannot assign to read only property 'x' of object '#<Object>'",false],["frozen","TypeError","Cannot assign to read only property 'x' of object '#<Object>'",false],["sealed","TypeError","Cannot add property x, object is not extensible",false],["sealed","TypeError","Cannot add property x, object is not extensible",false],["getter","TypeError","Cannot set property x of #<Object> which has only a getter",false],["getter","TypeError","Cannot set property x of #<Object> which has only a getter",false],["proxy","TypeError","'set' on proxy: trap returned falsish for property 'x'",false],["proxy-readonly","TypeError","Cannot assign to read only property 'x' of object '#<Object>'",false],["proxy-extend","TypeError","Cannot define property x, object is not extensible",false],["nested","TypeError","'set' on proxy: trap returned falsish for property 'x'",false],["setter-original",null,null,true],["hostile","TypeError","Cannot assign to read only property 'x' of object '[object Object]'",false],[["string","0"],"TypeError","Cannot assign to read only property '0' of string 'abc'",false],[["string","length"],"TypeError","Cannot assign to read only property 'length' of string 'abc'",false],[["string","5"],"TypeError","Cannot create property '5' on string 'abc'",false],["primitive-getter","TypeError","Cannot set property x of [object Boolean] which has only a getter",false],["reflect",false],["trap-get","trap-call","inner"]]"#]);
    }
}

#[test]
fn private_field_initialization_does_not_follow_proxy_prototypes() {
    let source = r#"
      const events=[];
      const prototype=new Proxy({}, {
        get(){events.push('get');throw Error('public get');},
        set(){events.push('set');throw Error('public set');},
        getOwnPropertyDescriptor(){events.push('descriptor');throw Error('public descriptor');},
        getPrototypeOf(){events.push('prototype');throw Error('public prototype');}
      });
      class ReturnValue { constructor(value){return value;} }
      class Stamp extends ReturnValue {
        #value;
        constructor(value){super(value);this.#value=42;}
        static read(value){return value.#value;}
        static has(value){return #value in value;}
      }
      const object=Object.create(prototype);
      const result=new Stamp(object);
      print(JSON.stringify([result===object,Stamp.has(object),Stamp.read(object),events]));
    "#;
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let program=compile(source,"private-proxy-prototype.js").unwrap();
        let output=Rc::new(RefCell::new(Vec::new()));
        let mut vm=Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(),["[true,true,42,[]]"]);
    }
}

#[test]
fn typed_array_base_methods_validate_backing_before_arguments_with_node_diagnostics() {
    let source = r#"
      for(const name of ['includes','indexOf','join','reverse','fill','copyWithin','slice','keys','values','entries']) {
        for(const detached of [false,true]) {
          const buffer=new ArrayBuffer(8,{maxByteLength:16}),view=new Uint8Array(buffer,4,2);
          if(detached)buffer.transfer();else buffer.resize(2);
          let called=false;
          const argument={valueOf(){called=true;throw Error('argument coercion');},toString(){called=true;throw Error('argument coercion');}};
          try{Uint8Array.prototype[name].call(view,argument);throw Error('accepted invalid backing');}
          catch(error){
            const state=name==='keys'?'detached':'detached or out-of-bounds';
            if(!(error instanceof TypeError)||error.message!==`Cannot perform %TypedArray%.prototype.${name} on a ${state} ArrayBuffer`)throw error;
          }
          if(called)throw Error('converted argument before validating receiver');
        }
        const value={toString(){throw Error('public conversion');}};
        try{Uint8Array.prototype[name].call(value);throw Error('accepted invalid brand');}
        catch(error){
          const expected=name==='join'||name==='slice'?`Method %TypedArray%.prototype.${name} called on incompatible receiver [object Object]`:'this is not a typed array.';
          if(!(error instanceof TypeError)||error.message!==expected)throw error;
        }
      }
    "#;
    for program in [
        Engine::compile(crate::ExecutionRequest {
            source,
            name: "typed-method-diagnostics.js",
            kind: crate::SourceKind::Script,
        })
        .unwrap(),
        Engine::specialize(source, "typed-method-diagnostics.js").unwrap(),
    ] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn owned_host_script_code_reclaims_after_owners_and_escaped_functions_drop() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize("void 0", "owned-code-lifetime.js").unwrap();
    vm.execute(&program).unwrap();
    let before = vm.programs.live_count();
    let script = vm.compile_embedding_host_script("({value:42})", "owned-object.js", (0, 0)).unwrap();
    let owner = Rc::downgrade(&script.state);
    vm.collect_now(&program);
    vm.collect_now(&program);
    let value = vm.evaluate_embedding_compiled_script(&script).unwrap();
    let root = vm.heap.root(value);
    vm.collect_now(&program);
    vm.collect_now(&program);
    assert!(vm.compiled_script_is_current(&script));
    drop(script);
    assert!(owner.upgrade().is_none());
    vm.heap.release_root(root);
    vm.collect_now(&program);
    vm.collect_now(&program);
    assert_eq!(vm.programs.live_count(), before);

    let script = vm.compile_embedding_host_script("(()=>41)", "escaped-owned.js", (0, 0)).unwrap();
    let function = vm.evaluate_embedding_compiled_script(&script).unwrap();
    let root = vm.heap.root(function);
    drop(script);
    vm.collect_now(&program);
    vm.collect_now(&program);
    assert!(vm.programs.live_count() > before);
    let function = vm.heap.root_value(root).unwrap();
    assert_eq!(vm.call_value(&program, function, Value::UNDEFINED, &[]).unwrap(), Value::number(41.0));
    vm.heap.release_root(root);
    vm.collect_now(&program);
    vm.collect_now(&program);
    assert_eq!(vm.programs.live_count(), before);
}

#[test]
fn function_native_info_lookup_transitions_match_in_both_execution_modes() {
    let source = r#"
Error.prepareStackTrace=(_,frames)=>frames;
const changes={name:f=>f.name,length:f=>f.length,reflect:f=>Reflect.get(f,'name'),computed:(f,k)=>f[k]};
for(const [kind,read] of Object.entries(changes)) {
 const fn=Function('return new Error().stack[0]');Object.setPrototypeOf({},fn);
 if(fn().getFunctionName()!=='anonymous')throw Error('setup '+kind);
 read(fn,'name');const expected=kind==='name'||kind==='length'?'eval':'anonymous';
 if(fn().getFunctionName()!==expected)throw Error('lookup '+kind);
}
const fn=Function('return new Error().stack[0]');delete fn.length;Object.setPrototypeOf({},fn);void fn.name;
if(fn().getFunctionName()!=='anonymous')throw Error('deleted length layout');
"#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "function-info-lookup.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "function-info-lookup.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn shared_number_arithmetic_maps_all_four_numeric_operators() {
    use oxc_ast::ast::BinaryOperator;
    use super::operations::NumberArithmetic;

    for (operator, expected) in [
        (BinaryOperator::Addition, NumberArithmetic::Add),
        (BinaryOperator::Subtraction, NumberArithmetic::Subtract),
        (BinaryOperator::Multiplication, NumberArithmetic::Multiply),
        (BinaryOperator::Division, NumberArithmetic::Divide),
    ] {
        assert_eq!(NumberArithmetic::from_operator(operator as u32), Some(expected));
    }
    assert_eq!(
        NumberArithmetic::from_operator(BinaryOperator::Equality as u32),
        None
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_hot_numeric_functions_and_guards_polymorphic_inputs() {
    let source = r#"
      function add(left, right) { return left + right; }
      function subtract(left, right) { return left - right; }
      function divide(left, right) { return left / right; }
      function plusOne(value) { return value + 1; }
      for (let index = 0; index < 40; index++) {
        if (add(index, 2) !== index + 2) throw new Error('numeric result');
        if (subtract(index + 2, 2) !== index) throw new Error('numeric subtraction result');
        if (divide(index + 1, 2) !== (index + 1) / 2) throw new Error('division result');
        if (plusOne(index) !== index + 1) throw new Error('numeric constant result');
      }
      if (add('value:', 2) !== 'value:2') throw new Error('guard fallback');
      if (plusOne('value') !== 'value1') throw new Error('constant string fallback');
      if (add(1n, 2n) !== 3n) throw new Error('BigInt fallback');
      let bigintAdditionThrew = false;
      try { plusOne(1n); } catch (error) { bigintAdditionThrew = error instanceof TypeError; }
      if (!bigintAdditionThrew) throw new Error('constant BigInt fallback');
      let coercions = 0;
      const boxed = { valueOf() { coercions++; return 3; } };
      if (add(boxed, 4) !== 7 || coercions !== 1) throw new Error('effectful fallback');
      if (plusOne(boxed) !== 4 || coercions !== 2) throw new Error('constant effectful fallback');
      if (!(new add(1, 2) instanceof add)) throw new Error('constructor unwind');
      if (!Object.is(divide(-0, 2), -0)) throw new Error('signed zero');
      if (!Number.isNaN(divide(0, 0))) throw new Error('NaN');
      if (divide(1, 0) !== Infinity) throw new Error('infinity');
    "#;
    let program = Engine::compile(crate::ExecutionRequest {
        source,
        name: "js-jit-number-guard.js",
        kind: crate::SourceKind::Script,
    })
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    for function in [1, 2, 3, 4] {
        assert!(matches!(
            vm.programs
                .js_tier(super::program_store::ProgramId::MAIN, function),
            Some(super::program_store::JsTierState::Compiled(_))
        ));
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_number_unary_plus_and_negation_with_exact_fallback() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function plus(value) { return +value; }
          function negate(value) { return -value; }
          const values = [0, -0, Infinity, -Infinity, NaN];
          for (let index = 0; index < 40; index++) {
            const value = values[index % values.length];
            if (!Object.is(plus(value), value)) throw Error('unary plus');
            if (!Object.is(negate(value), -value)) throw Error('unary negation');
          }
          if (plus('7') !== 7) throw Error('unary plus coercion fallback');
          let plusThrew = false;
          try { plus(7n); } catch (error) { plusThrew = error instanceof TypeError; }
          if (!plusThrew) throw Error('unary plus BigInt fallback');
          if (negate(7n) !== -7n) throw Error('unary negation BigInt fallback');
          let coercions = 0;
          const boxed = { valueOf() { coercions++; return 5; } };
          if (negate(boxed) !== -5 || coercions !== 1) throw Error('unary effectful fallback');
        "#,
        "js-jit-number-unary.js",
    ))
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    for name in ["plus", "negate"] {
        let function = program
            .functions
            .iter()
            .position(|function| {
                function
                    .header()
                    .name
                    .or(function.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32;
        let Some(super::program_store::JsTierState::Compiled(compiled)) =
            vm.programs
                .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("{name} should compile its Number unary denotation");
        };
        assert!(
            compiled.uses_shared_entry(),
            "{name} should use the common VM native-entry ABI"
        );
        assert!(
            compiled.native_entries() > 0,
            "{name} should have entered native code before the fallback checks"
        );
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_straight_line_plain_local_temporaries() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function scaled(value) {
            let temporary = value + 1;
            return temporary * 2;
          }
          for (let index = 0; index < 40; index++) {
            if (scaled(index) !== (index + 1) * 2) throw Error('local temporary result');
          }
          if (!Number.isNaN(scaled('x'))) throw Error('local temporary fallback');
          let coercions = 0;
          const boxed = { valueOf() { coercions++; return 5; } };
          if (scaled(boxed) !== 12 || coercions !== 1) throw Error('effectful fallback');
        "#,
        "js-jit-plain-local.js",
    ))
    .unwrap();
    let scaled = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "scaled")
        })
        .unwrap() as u32;
    assert!(
        super::js_compiled::compile(&program, scaled).is_some(),
        "the generic numeric lowering should accept a straight-line plain local"
    );

    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, scaled),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
}

/// A function the tier claims stays a frameless lane target: its activations start at the
/// tier prologue (`lane_tier_prologue`), which enters the compiled body and completes the
/// activation at its `Return`.
#[test]
#[cfg(not(quench_profiled))]
fn js_jit_enters_compiled_callees_from_frameless_lane_activations() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function add(left, right) { return left + right; }
          function drive(count) {
            let sum = 0;
            for (let index = 0; index < count; index++) sum = add(sum, index);
            return sum;
          }
          for (let round = 0; round < 4; round++) {
            if (drive(200) !== 19900) throw Error('frameless tier result');
          }
        "#,
        "js-jit-frameless-entry.js",
    ))
    .unwrap();
    let add = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|atom| &program.atoms[atom as usize] == "add")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, add)
    else {
        panic!("the numeric callee should be compiled");
    };
    assert!(
        compiled.native_entries() >= 500,
        "the lane's frameless calls should enter native code, got {}",
        compiled.native_entries()
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_routes_rejected_lane_candidates_through_the_general_frame_handoff() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function label(value) { return value + '!'; }
          var result = '';
          for (let index = 0; index < 200; index++) result = label(index);
          if (result !== '199!') throw Error('generic lane JIT result');
        "#,
        "js-jit-general-lane-entry.js",
    ))
    .unwrap();
    let label = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|atom| &program.atoms[atom as usize] == "label")
        })
        .unwrap() as u32;
    let body = program.function(label as usize);
    assert!(body.call_plan(&program).registers_only);
    assert!(super::js_compiled::function_candidate(body, &program));

    let mut vm = Vm::new(TestHost::default());
    vm.programs.enable_js_general_dispatch();
    vm.programs.compile_general_baseline_when_hot();
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    let tier = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, label);
    let Some(super::program_store::JsTierState::Compiled(compiled)) = tier else {
        let state = match tier {
            None => "untracked",
            Some(super::program_store::JsTierState::Hot { .. }) => "hot",
            Some(super::program_store::JsTierState::PromotionPending) => "promotion-pending",
            Some(super::program_store::JsTierState::GeneralPromotionPending { .. }) => {
                "general-promotion-pending"
            }
            Some(super::program_store::JsTierState::GeneralCompileDue) => "general-compile-due",
            Some(super::program_store::JsTierState::Rejected { .. }) => "rejected",
            Some(super::program_store::JsTierState::Unsupported) => "unsupported",
            Some(super::program_store::JsTierState::Compiled(_)) => "compiled-other-tier",
        };
        panic!("the lane callee should reach GeneralDispatch, got {state}");
    };
    assert!(compiled.is_general_dispatch());
    assert!(compiled.native_entries() > 0);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_forward_numeric_decision_trees() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function magnitude(value) {
            if (value < 0) return -value;
            return value;
          }
          function magnitudeJoined(value) {
            return value < 0 ? -value : value;
          }
          for (let index = 0; index < 40; index++) {
            if (magnitude(index - 20) !== Math.abs(index - 20)) throw Error('decision tree');
            if (magnitudeJoined(index - 20) !== Math.abs(index - 20)) throw Error('decision join');
          }
          if (!Object.is(magnitude(-0), -0)) throw Error('signed zero');
          if (!Number.isNaN(magnitude(NaN))) throw Error('NaN relation');
          if (magnitude('3') !== '3') throw Error('coercive fallback');
          let coercions = 0;
          const boxed = { valueOf() { coercions++; return -4; } };
          if (magnitude(boxed) !== 4 || coercions !== 2) throw Error('effectful fallback');
        "#,
        "js-jit-forward-decision-tree.js",
    ))
    .unwrap();
    let magnitude = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "magnitude")
        })
        .unwrap() as u32;
    let mut interpreter = Vm::new(TestHost::default());
    interpreter
        .execute(&program)
        .expect("interpreted decision tree should preserve JS results");
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    let compiled = match vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, magnitude)
    {
        Some(super::program_store::JsTierState::Compiled(compiled)) => compiled,
        Some(super::program_store::JsTierState::Rejected { reason, .. }) => {
            panic!("the Number decision tree was rejected: {reason}");
        }
        Some(super::program_store::JsTierState::Hot {
            observations,
            numeric,
        }) => panic!("the Number decision tree remained hot: {observations} observations, {numeric} numeric"),
        Some(super::program_store::JsTierState::PromotionPending) => {
            panic!("the Number decision tree remained promotion-pending");
        }
        Some(
            super::program_store::JsTierState::GeneralPromotionPending { .. }
            | super::program_store::JsTierState::GeneralCompileDue,
        ) => {
            panic!("the numeric decision tree was sent to the general baseline");
        }
        Some(super::program_store::JsTierState::Unsupported) => {
            panic!("the Number decision tree was marked unsupported");
        }
        None => panic!("the Number decision tree was not tier-tracked"),
    };
    assert!(compiled.uses_shared_entry());
    assert!(compiled.native_entries() > 0);

    let joined = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "magnitudeJoined")
        })
        .unwrap() as u32;
    match vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, joined)
    {
        Some(super::program_store::JsTierState::Compiled(compiled)) => {
            assert!(compiled.uses_shared_entry());
            assert!(compiled.native_entries() > 0);
        }
        Some(super::program_store::JsTierState::Rejected { reason, .. }) => {
            panic!("the Number join was rejected: {reason}");
        }
        Some(super::program_store::JsTierState::Hot {
            observations,
            numeric,
        }) => panic!("the Number join remained hot: {observations} observations, {numeric} numeric"),
        Some(super::program_store::JsTierState::PromotionPending) => {
            panic!("the Number join remained promotion-pending");
        }
        Some(
            super::program_store::JsTierState::GeneralPromotionPending { .. }
            | super::program_store::JsTierState::GeneralCompileDue,
        ) => {
            panic!("the numeric join was sent to the general baseline");
        }
        Some(super::program_store::JsTierState::Unsupported) => {
            panic!("the Number join was marked unsupported");
        }
        None => panic!("the Number join was not tier-tracked"),
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_enters_compiled_known_callees_without_pushing_a_frame() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function add(left, right) { return left + right; }
          for (let index = 0; index < 40; index++) {
            if (add(index, 2) !== index + 2) throw Error('known call result');
          }
        "#,
        "js-jit-known-call.js",
    ))
    .unwrap();
    let add = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "add")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, add)
    else {
        panic!("the numeric callee should be compiled");
    };
    assert!(
        compiled.uses_leaf_entry(),
        "the non-polling numeric artifact should use the shared leaf entry"
    );
    assert!(
        compiled.native_entries() >= 9,
        "promotion plus the remaining statically-known calls should enter native code"
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_number_capture_returns_and_observes_mutation() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function makeReader() {
            let value = 7;
            function read() { return value; }
            function write(next) { value = next; }
            return { read, write };
          }
          const state = makeReader();
          for (let index = 0; index < 40; index++) {
            if (state.read() !== 7) throw Error('capture number');
          }
          state.write(11);
          if (state.read() !== 11) throw Error('capture update');
          state.write('text');
          if (state.read() !== 'text') throw Error('non-number capture fallback');
        "#,
        "js-jit-capture-return.js",
    ))
    .unwrap();
    let reader = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "read")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, reader),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_numeric_capture_arithmetic_and_guards_mutation() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function makeAdder() {
            let delta = 3;
            function add(value) { return value + delta; }
            function update(next) { delta = next; }
            return { add, update };
          }
          const adder = makeAdder();
          for (let index = 0; index < 40; index++) {
            if (adder.add(index) !== index + 3) throw Error('numeric capture');
          }
          adder.update(5);
          if (adder.add(2) !== 7) throw Error('capture mutation');
          adder.update('x');
          if (adder.add(2) !== '2x') throw Error('capture fallback');
        "#,
        "js-jit-capture-arithmetic.js",
    ))
    .unwrap();
    let add = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "add")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, add),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_int32_bitwise_helpers_and_guards_the_domain() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function mask(value) { return value ^ 7; }
          for (let index = -20; index < 20; index++) {
            if (mask(index) !== (index ^ 7)) throw Error('int32 bitwise');
          }
          if (mask(2.5) !== (2 ^ 7)) throw Error('fraction fallback');
          if (mask(4294967296) !== 7) throw Error('range fallback');
          if (mask(4294967295) !== (-1 ^ 7)) throw Error('unsigned range fallback');
          if (mask(NaN) !== 7) throw Error('non-finite fallback');
          if (mask('13') !== 10) throw Error('coercion fallback');
          let bigintThrew = false;
          try { mask(13n); } catch (error) { bigintThrew = error instanceof TypeError; }
          if (!bigintThrew) throw Error('bigint fallback');
        "#,
        "js-jit-int32-bitwise.js",
    ))
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let mask = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "mask")
        })
        .unwrap() as u32;
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, mask),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_general_dispatch_compiles_tagged_int32_bitwise_ops_and_falls_back_exactly() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function bitAnd(left, right) {
            let result = 0;
            for (let pass = 0; pass < 2; pass++) result = left & right;
            return result;
          }
          function bitOr(left, right) {
            let result = 0;
            for (let pass = 0; pass < 2; pass++) result = left | right;
            return result;
          }
          function bitXor(left, right) {
            let result = 0;
            for (let pass = 0; pass < 2; pass++) result = left ^ right;
            return result;
          }
          function shiftLeft(left, right) {
            let result = 0;
            for (let pass = 0; pass < 2; pass++) result = left << right;
            return result;
          }
          function shiftRight(left, right) {
            let result = 0;
            for (let pass = 0; pass < 2; pass++) result = left >> right;
            return result;
          }
          function shiftRightUnsigned(left, right) {
            let result = 0;
            for (let pass = 0; pass < 2; pass++) result = left >>> right;
            return result;
          }
          var result = 0;
          for (let index = 0; index < 20000; index++) {
            result += bitAnd(index, 127) + bitOr(index, 32) + bitXor(index, 7);
            result += shiftLeft(index, 2) + shiftRight(-index, 1);
            result += shiftRightUnsigned(-1, index & 31);
          }
          if (bitAnd(6, 3) !== 2 || bitOr(6, 3) !== 7 || bitXor(6, 3) !== 5) {
            throw Error('tagged bitwise operation');
          }
          if (shiftLeft(-2, 2) !== -8 || shiftRight(-8, 2) !== -2) {
            throw Error('signed shift operation');
          }
          if (shiftRightUnsigned(-1, 0) !== 4294967295) {
            throw Error('unsigned shift must box outside int32');
          }
          let conversions = 0;
          const coercible = { valueOf() { conversions++; return 13; } };
          if (bitXor(coercible, 10) !== 7 || conversions !== 2) {
            throw Error('object coercion fallback');
          }
          if (bitXor(2.5, 7) !== 5 || bitXor(4294967296, 7) !== 7) {
            throw Error('numeric conversion fallback');
          }
          if (bitXor('13', 10) !== 7 || bitXor(NaN, 7) !== 7) {
            throw Error('primitive conversion fallback');
          }
          let bigintThrew = false;
          try { bitXor(13n, 1); } catch (error) { bigintThrew = error instanceof TypeError; }
          if (!bigintThrew) throw Error('mixed bigint fallback');
        "#,
        "js-general-int32-bitwise.js",
    ))
    .unwrap();

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.programs.compile_general_baseline_when_hot();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).unwrap();

    let result_atom = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_atom)
        .unwrap();
    let result = compiled
        .own_property(compiled.realm.globals, result_atom)
        .unwrap();
    assert_eq!(result, expected);

    for name in [
        "bitAnd",
        "bitOr",
        "bitXor",
        "shiftLeft",
        "shiftRight",
        "shiftRightUnsigned",
    ] {
        let function = program
            .functions
            .iter()
            .position(|body| {
                body.header()
                    .name
                    .or(body.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32;
        let Some(super::program_store::JsTierState::Compiled(native)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("{name} did not compile through GeneralDispatch");
        };
        assert!(native.is_general_dispatch(), "{name} used a specialized compiler");
        assert!(native.native_entries() > 0, "{name} has no native entries");
        let body = program.function_get(function as usize).unwrap();
        let binary_pcs: Vec<_> = body
            .code
            .iter()
            .enumerate()
            .filter_map(|(pc, instruction)| {
                (instruction.as_wide().op() == crate::bytecode::Op::Binary).then_some(pc as u32)
            })
            .collect();
        assert!(!binary_pcs.is_empty());
        for pc in binary_pcs {
            assert!(
                native.general_dispatch_pc_is_native(pc as usize),
                "{name} pc {pc} is a callout"
            );
        }
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_guarded_int32_identity_arm_and_falls_back_for_other_types() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function hashKey(key) {
            switch (typeof key) {
              case 'object':
                if (!key) return 0;
                return key.hashCode();
              case 'number':
                if ((key | 0) == key) return key;
                return 'fraction';
              default: return 'other';
            }
          }
          function exercise() {
            for (let index = 0; index < 40; index++) {
              if (hashKey(index) !== index) throw Error('integer fast arm');
            }
            if (hashKey(2.5) !== 'fraction') throw Error('fraction fallback');
            if (hashKey('2') !== 'other') throw Error('type fallback');
            if (hashKey({ hashCode() { return 7; } }) !== 7) throw Error('object path');
          }
          exercise();
        "#,
        "js-jit-int32-identity.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "hashKey")
        })
        .unwrap() as u32;
    assert!(
        super::js_compiled::int32_identity_return_candidate(
            &program,
            program.function_get(function_id as usize).unwrap()
        )
        .is_some(),
        "{}",
        program.disassemble()
    );
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, function_id),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function_id)
    else {
        unreachable!("the guarded identity function was compiled above")
    };
    assert!(compiled.native_entries() >= 9);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_pure_numeric_loops_with_backedge_polls() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function sumTo(limit) {
            let sum = 0;
            let index = 0;
            while (index < limit) {
              sum = sum + index;
              index = index + 1;
            }
            return sum;
          }
          function capacity(x) {
            if (x >= 1073741824) return 1073741824;
            if (x === 0) return 16;
            x = x - 1;
            x |= x >> 1;
            x |= x >> 2;
            x |= x >> 4;
            x |= x >> 8;
            x |= x >> 16;
            return x + 1;
          }
          if (sumTo(39) !== 741) throw Error('numeric loop result');
          for (let index = 0; index < 40; index++) {
            const capacityExpected = index === 0 ? 16 : index <= 2 ? index : index <= 4 ? 4 : index <= 8 ? 8 : index <= 16 ? 16 : index <= 32 ? 32 : 64;
            if (capacity(index) !== capacityExpected) throw Error('bitwise loop result ' + index + ' ' + capacity(index) + ' ' + capacityExpected);
          }
          if (capacity(NaN) !== 1 || capacity(Infinity) !== 1073741824 || capacity(-Infinity) !== 1) {
            throw Error('bitwise non-finite conversion');
          }
          if (capacity(-4294967297) !== 0 || capacity(-1) !== 0 || capacity(-Number.MAX_VALUE) !== 1) {
            throw Error('bitwise modulo conversion');
          }
        "#,
        "js-jit-numeric-loop.js",
    ))
    .unwrap();
    assert!(
        super::js_compiled::compile(&program, 1).is_some(),
        "direct compile failed\n{}",
        program.disassemble()
    );
    assert!(
        super::js_compiled::function_candidate(program.function_get(2).expect("capacity function"), &program),
        "capacity function candidate rejected"
    );
    assert!(
        super::js_compiled::compile(&program, 2).is_some(),
        "direct bitwise compile failed\n{}",
        program.disassemble()
    );
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(
        matches!(
            vm.programs
                .js_tier(super::program_store::ProgramId::MAIN, 1),
            Some(super::program_store::JsTierState::Compiled(_))
        ),
        "{}",
        program.disassemble()
    );
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, 2),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_interrupt_exits_through_the_shared_native_entry() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function sumTo(limit) {
            let sum = 0;
            let index = 0;
            while (index < limit) {
              sum = sum + index;
              index = index + 1;
            }
            return sum;
          }
        "#,
        "js-jit-interrupt-adapter.js",
    ))
    .unwrap();
    let artifact = super::js_compiled::compile(&program, 1).expect("numeric loop artifact");
    assert!(matches!(
        artifact.result,
        super::js_compiled::JsJitResultKind::NumberInterruptible
    ));
    let compiled = super::compiled::CompiledJsFunction::from_artifact(artifact)
        .expect("shared executable mapping");
    assert!(
        !compiled.uses_leaf_entry(),
        "an interruptible loop must retain the resumable native entry"
    );
    let interrupt = std::sync::atomic::AtomicBool::new(true);

    let result = compiled.call(&[1_000_000.0], &interrupt, None);

    assert!(result.is_none(), "interrupted native execution must not return a guest result");
    assert_eq!(compiled.native_entries(), 1, "the shared adapter must enter once");
    assert_eq!(compiled.specialized_guard_misses(), 0, "interrupts are native exits, not entry guard misses");
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_repeated_specialized_guard_misses_release_cache_for_general_tier() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        "function add(left, right) { return left + right; }",
        "js-jit-specialized-guard-promotion.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "add")
        })
        .unwrap() as u32;
    let artifact = super::js_compiled::compile(&program, function).expect("numeric add artifact");
    let compiled = Rc::new(
        super::compiled::CompiledJsFunction::from_artifact(artifact)
            .expect("executable specialized artifact"),
    );
    let bytes = compiled.mapped_bytes();
    let mut programs = super::program_store::ProgramStore::default();
    programs.enable_js_general_dispatch();
    programs.compile_general_baseline_when_hot();
    let id = programs.insert(program).unwrap();
    assert!(programs.install_js_tier(id, function, Some(compiled), false));
    assert_eq!(programs.jit_mapped_bytes(), bytes);

    for _ in 0..super::program_store::JS_JIT_SPECIALIZED_GUARD_MISS_THRESHOLD {
        let due = programs.note_js_specialized_guard_miss(id, function, 5);
        assert_eq!(
            due,
            programs.js_tier(id, function).is_some_and(|state| matches!(state, super::program_store::JsTierState::GeneralCompileDue)),
            "the tier should become due exactly when the miss threshold is reached"
        );
    }
    assert!(matches!(
        programs.js_tier(id, function),
        Some(super::program_store::JsTierState::GeneralCompileDue)
    ));
    assert_eq!(
        programs.jit_mapped_bytes(),
        0,
        "the specialized mapping must stop consuming shared JIT cache budget at demotion"
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_counts_only_tagged_guard_misses_after_falsy_native_success() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function add(left, right) { return left + right; }
          function read(scale, node) { return node.value * scale; }
          for (let index = 0; index < 40; index++) {
            add(index, 0);
            read(2, { value: index });
          }
          if (add(0, 0) !== 0) throw Error('falsy numeric result');
          if (add('tag', 1) !== 'tag1') throw Error('tagged fallback');
          new add('frame', 1);
          let gets = 0;
          const accessor = { get value() { gets++; return 3; } };
          if (read(2, accessor) !== 6 || gets !== 1) throw Error('shape fallback');
        "#,
        "js-jit-specialized-guard-miss-count.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "add")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.programs.enable_js_general_dispatch();
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("one confirmed miss should not yet replace the specialized tier");
    };
    assert!(!compiled.is_general_dispatch());
    assert_eq!(
        compiled.specialized_guard_misses(),
        2,
        "numeric zero must not count, and each frameless/frame miss must count once"
    );
    let read = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "read")
        })
        .unwrap() as u32;
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, read)
    else {
        panic!("the numeric field reader should retain its specialized tier after one miss");
    };
    assert!(!compiled.is_general_dispatch());
    assert_eq!(compiled.specialized_guard_misses(), 1, "shape guard miss count");
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_specialized_guard_misses_handoff_to_general_dispatch() {
    let source = format!(
        r#"
          function add(left, right) {{ return left + right; }}
          for (let index = 0; index < 40; index++) add(index, 1);
          for (let index = 0; index < {misses}; index++) {{
            const expected = 'tag' + index;
            if (add('tag', index) !== expected) throw Error('tagged interpreter fallback');
          }}
          const result = add('last', 1);
          if (result !== 'last1') throw Error('general tier result');
          print(result);
        "#,
        misses = super::program_store::JS_JIT_SPECIALIZED_GUARD_MISS_THRESHOLD,
    );
    let program = Engine::compile(crate::ExecutionRequest::script(
        &source,
        "js-jit-specialized-guard-general-dispatch.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "add")
        })
        .unwrap() as u32;
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = Vm::new(TestHost::writing_to(interpreted_output.clone()));
    interpreter.execute(&program).unwrap();

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(compiled_output.clone()));
    vm.programs.enable_js_general_dispatch();
    vm.programs.compile_general_baseline_when_hot();
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice());
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("repeated tagged guard misses should promote the function");
    };
    assert!(compiled.is_general_dispatch());
    assert!(compiled.native_entries() > 0, "the follow-up call should run natively");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_a_guarded_numeric_field_loop() {
    let source = r#"
      function sum(item) {
        let total = 0;
        for (let index = 0; index < item.limit; index++) total += index;
        return total;
      }
      const item = { limit: 10000 };
      let answer = 0;
      for (let call = 0; call < 100; call++) answer += sum(item);
      print(answer);
    "#;
    let program = Engine::specialize(source, "osr-numeric-field.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "sum")
        })
        .unwrap();

    // Each policy compiles the loop at its own optimization level, or not at all
    // (`LOOP_TIER_OPT_LEVELS`); the result is the same either way.
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.execute(&program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), ["4999500000"], "{policy:?}");
        let views = compiled
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let view = views
            .get(function_id)
            .and_then(super::program_store::derived_lane_view)
            .expect("hot sum loop lane view");
        let osr = view.osr_diagnostics();
        // Loop sites are analyzed only where the JavaScript tiers run (`JS_FUNCTION_TIERS`).
        let analyzes = super::compiled::js_function_tiers_compile(policy);
        assert_eq!(osr.admitted_sites, usize::from(analyzes), "field loop admission under {policy:?}");
        let compiles = super::compiled::loop_tier_compiles(policy);
        assert_eq!(osr.currently_compiled_sites, usize::from(compiles), "field loop native code under {policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reads_fields_in_a_loop_that_stores_elements() {
    // The loop reads `src.n`, `src.scale` and a named property of the very array it stores
    // into: in-bounds element stores leave every named property as it was.
    let source = r#"
      function fill(src, dst) {
        for (let index = 0; index < src.n; index++) dst[index] = index * src.scale + dst.bias;
        return dst[0] + dst[src.n - 1];
      }
      const dst = new Array(4096).fill(0);
      dst.bias = 7;
      const src = { n: 4000, scale: 3 };
      let answer = 0;
      for (let call = 0; call < 100; call++) answer += fill(src, dst);
      print(answer);
    "#;
    let program = Engine::specialize(source, "osr-field-store.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "fill"))
        .unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.execute(&program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), ["1201100"], "{policy:?}");
        let views = compiled
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let osr = views
            .get(function_id)
            .and_then(super::program_store::derived_lane_view)
            .expect("hot fill loop lane view")
            .osr_diagnostics();
        // Loop sites are analyzed only where the JavaScript tiers run (`JS_FUNCTION_TIERS`).
        let analyzes = super::compiled::js_function_tiers_compile(policy);
        assert_eq!(osr.admitted_sites, usize::from(analyzes), "field-reading store loop admission under {policy:?}");
        let compiles = super::compiled::loop_tier_compiles(policy);
        assert_eq!(osr.currently_compiled_sites, usize::from(compiles), "store loop native code under {policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_stores_guarded_numeric_own_fields_in_a_loop() {
    let source = r#"
      function fill(item, limit) {
        for (let index = 0; index < limit; index++) {
          item.value = (index + 1) * 0.5;
          item.other = index + 1;
        }
        return limit;
      }
      function fillReference(item, value, limit) {
        for (let index = 0; index < limit; index++) item.reference = value;
        return item.reference === value ? limit : -1;
      }
      const item = { value: 0, other: 0, left: 1, right: 2 };
      const reference = {};
      const referenceTarget = { reference: null, left: 1, right: 2 };
      const frozen = { value: 0, other: 0, left: 1, right: 2 };
      let answer = 0;
      for (let call = 0; call < 80; call++) answer += fill(item, 2000);
      fill(frozen, 3);
      Object.freeze(frozen);
      fill(frozen, 4);
      answer += fillReference(referenceTarget, reference, 2000);
      let setterCalls = 0;
      Object.defineProperty(item, "value", {
        set(value) { setterCalls++; }, configurable: true
      });
      answer += fill(item, 10);
      let proxyCalls = 0;
      const proxyTarget = { value: 0, other: 0, left: 1, right: 2 };
      const proxy = new Proxy(proxyTarget, {
        set(target, key, value) { proxyCalls++; target[key] = value; return true; }
      });
      fill(proxy, 5);
      print(answer + " " + setterCalls + " " + proxyCalls + " " +
        (referenceTarget.reference === reference) + " " + item.other + " " + proxyTarget.other + " " + frozen.other);
    "#;
    let program = Engine::specialize(source, "osr-field-store-numeric.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|atom| &program.atoms[atom as usize] == "fill")
        })
        .unwrap();

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = Vm::new(TestHost::writing_to(interpreted_output.clone()));
    interpreter.set_execution_mode(crate::ExecutionMode::Interpreted);
    interpreter.execute(&program).unwrap();

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
    compiled.set_memory_policy(crate::MemoryPolicy::Throughput);
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.enable_js_jit_diagnostics();
    compiled.execute(&program).unwrap();

    assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice());
    assert_eq!(compiled_output.borrow().as_slice(), ["162010 10 10 true 10 5 3"]);
    let views = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let osr = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot field-store loop lane view")
        .osr_diagnostics();
    assert!(osr.currently_compiled_sites > 0, "field-store loop did not compile: {osr:?}");
    assert!(osr.native_region_runs > 0, "field-store loop never ran natively: {osr:?}");
    let reference_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|atom| &program.atoms[atom as usize] == "fillReference")
        })
        .unwrap();
    let reference_osr = views
        .get(reference_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot reference field-store loop lane view")
        .osr_diagnostics();
    assert!(reference_osr.currently_compiled_sites > 0, "reference field-store loop did not compile: {reference_osr:?}");
    assert!(reference_osr.native_region_runs > 0, "reference field-store loop never ran natively: {reference_osr:?}");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_appends_and_fills_holes_in_place() {
    // The loops append at the length while the backing has room and fill holes below it.
    // Once a prototype holds an index, the entry declines, and the setter it defines runs.
    let source = r#"
      function grow(n) {
        var a = [];
        for (var i = 0; i < n; i++) a[i] = i * 3;
        var h = [];
        h[n - 1] = 1;
        for (var j = n - 2; j >= 0; j--) h[j] = j & 7;
        var s = 0;
        for (var k = 0; k < n; k++) s = (s + a[k] + h[k]) | 0;
        return s + " " + a.length + " " + h.length;
      }
      var out;
      for (var c = 0; c < 200; c++) out = grow(2000);
      var seen = 0;
      Object.defineProperty(Array.prototype, 5, { set(v) { seen += v + 1; }, configurable: true });
      var late = grow(2000);
      delete Array.prototype[5];
      print(out + " | " + late + " " + seen);
    "#;
    let program = Engine::specialize(source, "osr-append.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "grow"))
        .unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        compiled.execute(&program).unwrap();

        assert_eq!(output.borrow().as_slice(), ["6003994 2000 2000 | 6003934 2000 2000 22"], "{policy:?}");
        if !super::compiled::loop_tier_compiles(policy) {
            continue;
        }
        let views = compiled
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let osr = views
            .get(function_id)
            .and_then(super::program_store::derived_lane_view)
            .expect("hot grow loop lane view")
            .osr_diagnostics();
        assert!(osr.native_region_runs > 0, "no native run under {policy:?}: {osr:?}");
        // A store leaves only where the backing is full, once per capacity growth.
        assert!(osr.exits < osr.native_region_runs, "appends left native code under {policy:?}: {osr:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_indexes_arrays_that_field_reads_give() {
    // Each iteration reads the arrays from fields of `this` and of an argument, then reads
    // and stores their elements: the arrays are those the fields held at entry.
    let source = r#"
      function B(n) { this.arr = []; for (var k = 0; k < n; k++) this.arr.push(k & 255); }
      B.prototype.mix = function mix(w, n) {
        var s = 0;
        for (var i = 0; i < n; i++) {
          var a = this.arr;
          var b = w.arr;
          s = (s + a[i] * 3 + b[i]) | 0;
          b[i] = (a[i] + i) & 1023;
        }
        return s;
      };
      var x = new B(3000), y = new B(3000), t = 0;
      for (var r = 0; r < 300; r++) t = (t + x.mix(y, 3000)) | 0;
      print(t + " " + y.arr[17] + " " + y.arr[2999]);
    "#;
    let program = Engine::specialize(source, "osr-field-arrays.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "mix"))
        .unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.execute(&program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), ["804092460 34 110"], "{policy:?}");
        let views = compiled
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let osr = views
            .get(function_id)
            .and_then(super::program_store::derived_lane_view)
            .expect("hot mix loop lane view")
            .osr_diagnostics();
        // Loop sites are analyzed only where the JavaScript tiers run (`JS_FUNCTION_TIERS`).
        let analyzes = super::compiled::js_function_tiers_compile(policy);
        assert_eq!(osr.admitted_sites, usize::from(analyzes), "field-array loop admission under {policy:?}");
        let compiles = super::compiled::loop_tier_compiles(policy);
        assert_eq!(osr.currently_compiled_sites, usize::from(compiles), "field-array loop native code under {policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reports_captured_environment_store_rejection_by_site() {
    // A nested closure retains the mutable binding, so the loop reads and updates it through
    // the environment. Guarded reads are supported; stores still reject before native entry.
    let source = r#"
      function run() {
        let total = 0;
        const read = () => total;
        while (total < 2000) total += 1;
        return total + read();
      }
      let result = 0;
      for (let repeat = 0; repeat < 48; repeat++) result = run();
      print(result);
    "#;
    let program = Engine::specialize(source, "osr-captured-env-store-rejection.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "run")
        })
        .unwrap();

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone()))
        .execute(&program)
        .unwrap();

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
    compiled.set_memory_policy(crate::MemoryPolicy::Throughput);
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.enable_js_jit_diagnostics();
    compiled.execute(&program).unwrap();
    assert_eq!(
        compiled_output.borrow().as_slice(),
        interpreted_output.borrow().as_slice()
    );
    assert_eq!(compiled_output.borrow().as_slice(), ["4000"]);

    let views = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .unwrap();
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("captured loop lane view");
    let osr = view.osr_diagnostics();
    assert_eq!(
        osr.sites.len(),
        1,
        "one source loop should produce one attributed OSR site: {osr:?}"
    );
    let site = &osr.sites[0];
    assert_eq!(osr.admitted_sites, 0, "rejected candidates are not admitted OSR sites: {osr:?}");
    assert!(
        !site.admitted,
        "StoreEnvLocal region must reject before native admission: {site:?}"
    );
    assert!(matches!(
        site.analysis_rejection,
        Some(super::compiled::RegionRejection::UnsupportedInstruction(
            crate::bytecode::Op::StoreEnvLocal
        ))
    ), "StoreEnvLocal should be the first blocking operation: {site:?}");
    assert_eq!(site.compile_attempts, 0);
    assert_eq!(site.native_region_runs, 0);
    assert!(
        site.lane_backedge_count >= u64::from(super::dispatch_fast::LOOP_TIER_THRESHOLD),
        "the diagnostic fixture must execute the rejected loop: {site:?}"
    );

    let body = program.function_get(function_id).unwrap();
    let environment_store_pcs = body
        .code
        .iter()
        .enumerate()
        .filter_map(|(pc, packed)| {
            let instruction = super::dispatch_fast::decoded_instruction(body, *packed);
            (instruction.op() == crate::bytecode::Op::StoreEnvLocal
                && site.header_pc as usize <= pc
                && pc < site.backedge_pc as usize)
                .then_some(pc as u32)
        })
        .collect::<Vec<_>>();
    assert!(
        !environment_store_pcs.is_empty(),
        "the rejected bytecode range must contain StoreEnvLocal: {site:?}"
    );
    assert!(
        site.failure.is_some_and(|failure| environment_store_pcs.contains(&failure.pc)),
        "the reported rejection PC must identify the blocking StoreEnvLocal: {site:?}"
    );
    assert_eq!(
        super::dispatch_fast::decoded_instruction(body, body.code[site.backedge_pc as usize]).op(),
        crate::bytecode::Op::Jump
    );
    compiled.programs.record_js_jit_lane_entry(
        super::program_store::ProgramId::MAIN,
        function_id as u32,
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reports_pino_like_deferred_calls_per_site_with_gc_and_throw() {
    // Five distinct loops call a polymorphic callback. Each reaches the OSR threshold, but
    // no callback identity is stable enough to build an inlined native region. The callbacks
    // force collection repeatedly and throw one unique object at the final call.
    let source = r#"
      const marker = {};
      let calls = 0;
      function effectA(value) {
        calls++;
        if (calls % 64 === 0) $262.gc();
        if (calls === 5050) throw marker;
        return value + 1;
      }
      function effectB(value) {
        calls++;
        if (calls % 64 === 0) $262.gc();
        if (calls === 5050) throw marker;
        return value + 1;
      }
      function run(effect) {
        let caught = false;
        let total = 0;
        try {
          let a = 0; while (a < 10) a = effect(a); total += a;
          let b = 0; while (b < 10) b = effect(b); total += b;
          let c = 0; while (c < 10) c = effect(c); total += c;
          let d = 0; while (d < 10) d = effect(d); total += d;
          let e = 0; while (e < 10) e = effect(e); total += e;
        } catch (error) {
          caught = error === marker;
        }
        return total + " " + calls + " " + caught;
      }
      let result = "";
      for (let repeat = 0; repeat < 101; repeat++) result = run(repeat & 1 ? effectA : effectB);
      print(result);
    "#;
    let program = Engine::specialize(source, "osr-deferred-callback-gc-throw.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "run")
        })
        .unwrap();

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone()).with_test262())
        .execute(&program)
        .unwrap();

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()).with_test262());
    compiled.set_memory_policy(crate::MemoryPolicy::Throughput);
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.enable_js_jit_diagnostics();
    compiled.execute(&program).unwrap();
    assert_eq!(
        compiled_output.borrow().as_slice(),
        interpreted_output.borrow().as_slice()
    );
    assert_eq!(compiled_output.borrow().as_slice(), ["40 5050 true"]);

    let views = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .unwrap();
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("callback loop lane view");
    let osr = view.osr_diagnostics();
    assert_eq!(
        osr.sites.len(),
        5,
        "all five source loops need distinct OSR PCs: {osr:?}"
    );
    assert_eq!(osr.admitted_sites, 5, "all five call-prefix regions are admitted: {osr:?}");
    for site in &osr.sites {
        assert!(
            site.admitted && site.deferred_call,
            "dynamic callback should defer region compilation: {site:?}"
        );
        assert!(
            site.tier_threshold_reached
                && site.lane_backedge_count >= u64::from(super::dispatch_fast::LOOP_TIER_THRESHOLD)
        );
        assert_eq!(site.analysis_rejection, None);
        let body = program.function_get(function_id).unwrap();
        let call_pcs = body
            .code
            .iter()
            .enumerate()
            .filter_map(|(pc, packed)| {
                (site.header_pc as usize <= pc
                    && pc <= site.backedge_pc as usize
                    && super::dispatch_fast::decoded_instruction(body, *packed).op()
                        == crate::bytecode::Op::Call)
                    .then_some(pc as u32)
            })
            .collect::<Vec<_>>();
        assert_eq!(call_pcs, site.call_boundary_pcs, "call boundary attribution: {site:?}");
        assert_eq!(call_pcs.len(), 1, "one deferred callback per loop: {site:?}");
        assert!(
            site.compile_attempts > 0 && site.native_region_runs > 0 && site.exits > 0,
            "the call-free prefix must execute natively then leave for the callback: {site:?}"
        );
    }
    compiled.programs.record_js_jit_lane_entry(
        super::program_store::ProgramId::MAIN,
        function_id as u32,
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_moves_tagged_array_values_before_effectful_calls() {
    // Copying the array into a callback argument is a tagged Move; its call is an exact-PC
    // boundary, so the callback may mutate the array before the next native entry reloads it.
    let source = r#"
      let calls = 0;
      function effect(array) {
        calls++;
        if (calls === 1001) array[0] = 100;
      }
      function run(array, callback) {
        for (let index = 0; index < 2000; index++) {
          const alias = array;
          callback(alias);
        }
        return array[0];
      }
      const values = [1, 1];
      let result = 0;
      for (let repeat = 0; repeat < 25; repeat++) result = run(values, effect);
      print(result + " " + calls);
    "#;
    let program = Engine::specialize(source, "osr-tagged-array-move-call-boundary.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "run")
        })
        .unwrap();

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone()))
        .execute(&program)
        .unwrap();

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
    compiled.set_memory_policy(crate::MemoryPolicy::Throughput);
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.enable_js_jit_diagnostics();
    compiled.execute(&program).unwrap();
    assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice());
    assert_eq!(compiled_output.borrow().as_slice(), ["100 50000"]);

    let view = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .unwrap()
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot tagged-Move callback loop lane view");
    let osr = view.osr_diagnostics();
    assert_eq!(osr.sites.len(), 1, "one loop must have one OSR site: {osr:?}");
    let site = &osr.sites[0];
    assert!(site.admitted && site.deferred_call, "the call is a resumable exact-PC boundary: {site:?}");
    assert!(site.compile_attempts > 0 && site.native_region_runs > 0 && site.exits > 0, "tagged Move should run natively before its call: {site:?}");
    assert_eq!(site.call_boundary_pcs.len(), 1, "one dynamic callback site: {site:?}");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reads_tagged_dense_values_and_falls_back_on_array_holes() {
    // A dense indexed read may produce any JS value. Native code can carry that boxed word
    // through a Move, but a hole must leave at GetIndex so prototype lookup stays in the VM.
    let source = r#"
      let calls = 0;
      let sawHole = false;
      const values = [{ kind: "present" }];
      function effect(value) {
        calls++;
        if (calls === 1001) delete values[0];
        if (value === undefined) sawHole = true;
      }
      function run(array, callback) {
        for (let index = 0; index < 2000; index++) {
          const value = array[0];
          callback(value);
        }
      }
      for (let repeat = 0; repeat < 25; repeat++) run(values, effect);
      print(sawHole + " " + calls);
    "#;
    let program = Engine::specialize(source, "osr-tagged-array-read-hole-call-boundary.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "run")
        })
        .unwrap();

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone()))
        .execute(&program)
        .unwrap();

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
    compiled.set_memory_policy(crate::MemoryPolicy::Throughput);
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.enable_js_jit_diagnostics();
    compiled.execute(&program).unwrap();
    assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice());
    assert_eq!(compiled_output.borrow().as_slice(), ["true 50000"]);

    let view = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .unwrap()
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot tagged-index callback loop lane view");
    let osr = view.osr_diagnostics();
    assert_eq!(osr.sites.len(), 1, "one loop must have one OSR site: {osr:?}");
    let site = &osr.sites[0];
    assert!(site.admitted && site.deferred_call, "the callback remains an exact-PC boundary: {site:?}");
    assert!(site.compile_attempts > 0 && site.native_region_runs > 0, "tagged indexed reads should execute natively: {site:?}");
    assert!(site.exits > 0, "the dynamic call and hole fallbacks must leave the native region: {site:?}");
    assert_eq!(site.call_boundary_pcs.len(), 1, "one dynamic callback site: {site:?}");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_exact_effect_exit_preserves_prefix_across_gc_and_throw() {
    // The hot loop has a long numeric prefix and one call site whose callee collects and
    // throws a unique object. An exact-PC exit must preserve the numeric prefix, run the
    // ordinary call once, and deliver that same thrown object to the surrounding catch.
    let source = r#"
      const marker = {};
      let calls = 0;
      function effect(value) {
        calls++;
        $262.gc();
        if (value === 1040) throw marker;
        return value;
      }
      function run() {
        let total = 0;
        let caught = false;
        try {
          for (let index = 0; index < 1100; index++) {
            total += index;
            if (index === 1040) effect(index);
          }
        } catch (error) {
          caught = error === marker;
        }
        return total + " " + calls + " " + caught;
      }
      print(run());
    "#;
    let program = Engine::specialize(source, "osr-exact-effect-exit-gc-throw.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "run")
        })
        .unwrap();

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone()).with_test262())
        .execute(&program)
        .unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["541320 1 true"]);

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()).with_test262());
    compiled.set_memory_policy(crate::MemoryPolicy::Throughput);
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.enable_js_jit_diagnostics();
    compiled.execute(&program).unwrap();
    assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice());

    let views = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .unwrap();
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("effectful loop lane view");
    let osr = view.osr_diagnostics();
    assert_eq!(osr.sites.len(), 1, "one loop must have one OSR site: {osr:?}");
    if std::env::var_os("QUENCH_JS_JIT_STATS").is_some() {
        assert!(
            osr.native_region_runs > 0 && osr.exits > 0,
            "the forced effect boundary must run and exit native code: {osr:?}"
        );
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_inlines_method_calls_in_a_loop() {
    // Each iteration calls `x.am` twice: the region runs am3 in place, its object
    // parameter bound to the caller register it was moved from.
    let source = r#"
function am3(i,x,w,j,c,n) {
  var this_array = this.array;
  var w_array    = w.array;
  var xl = x&0x3fff, xh = x>>14;
  while(--n >= 0) {
    var l = this_array[i]&0x3fff;
    var h = this_array[i++]>>14;
    var m = xh*l+h*xl;
    l = xl*l+((m&0x3fff)<<14)+w_array[j]+c;
    c = (l>>28)+(m>>14)+xh*h;
    w_array[j++] = l&0xfffffff;
  }
  return c;
}
function B(n) { this.array = []; this.t = n; for (var k = 0; k < n; k++) this.array.push((k * 2654435761) & 0xfffffff); }
B.prototype.am = am3;
function sq(x, r) {
  var x_array = x.array, r_array = r.array, s = 0;
  for (var i = 0; i < x.t - 1; ++i) {
    var c = x.am(i, x_array[i], r, 2 * i, 0, 1);
    s = (s + c + x.am(i + 1, 2 * x_array[i], r, 2 * i + 1, c, x.t - i - 1)) | 0;
  }
  return s;
}
var a = new B(40), w = new B(100), t = 0;
for (var q = 0; q < 3000; q++) t = (t + sq(a, w)) | 0;
print(t + " " + w.array[5] + " " + w.array[77]);

    "#;
    let program = Engine::specialize(source, "osr-inline-calls.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "sq"))
        .unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        compiled.execute(&program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
        let views = compiled
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let osr = views
            .get(function_id)
            .and_then(super::program_store::derived_lane_view)
            .expect("hot caller loop lane view")
            .osr_diagnostics();
        let compiles = super::compiled::loop_tier_compiles(policy);
        assert_eq!(osr.currently_compiled_sites, usize::from(compiles), "inlining loop native code under {policy:?}");
        assert_eq!(osr.native_region_runs > 0, compiles, "inlining loop native runs under {policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_inlined_callee_leaves_through_its_activation() {
    // An element the inlined callee reads leaves the subset partway through its loop: native
    // code leaves at the callee's instruction, whose activation the interpreter finishes.
    let source = r#"
function addRun(src, w, j, n, c) {
  var w_array = w.arr, s_array = src.arr;
  var s = c;
  while (--n >= 0) {
    s = s + s_array[j];
    w_array[j++] = s & 1023;
  }
  return s;
}
function Box(n) { this.arr = []; for (var k = 0; k < n; k++) this.arr.push(k % 13); }
Box.prototype.run = addRun;
function drive(x, src, w, rounds) {
  var t = 0;
  for (var i = 0; i < rounds; i++) {
    var c = x.run(src, w, i & 31, 20, i);
    t = (t + (c | 0)) | 0;
  }
  return t;
}
var x = new Box(10), src = new Box(100), w = new Box(100), total = 0;
for (var q = 0; q < 300; q++) total = (total + drive(x, src, w, 200)) | 0;
src.arr[40] = 3.25;     // doubles from here on: widening inside the callee
for (var q = 0; q < 20; q++) total = (total + drive(x, src, w, 200)) | 0;
src.arr[44] = "9";      // a string: every pass reaching it leaves mid-callee
for (var q = 0; q < 20; q++) total = (total + drive(x, src, w, 200)) | 0;
print(total + " " + w.arr.slice(35, 50).join(","));

    "#;
    let program = Engine::specialize(source, "osr-inline-exits.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "drive"))
        .unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        compiled.execute(&program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
        let views = compiled
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let osr = views
            .get(function_id)
            .and_then(super::program_store::derived_lane_view)
            .expect("hot caller loop lane view")
            .osr_diagnostics();
        let compiles = super::compiled::loop_tier_compiles(policy);
        assert_eq!(osr.currently_compiled_sites, usize::from(compiles), "inlining loop native code under {policy:?}");
        assert_eq!(osr.native_region_runs > 0, compiles, "inlining loop native runs under {policy:?}");
        if compiles {
            assert!(osr.exits > 0, "no exit left an inlined callee under {policy:?}");
        }
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_inlines_calls_on_a_receiver_a_field_read_gives() {
    // `reduce` calls `this.m.am(…)` and reads `this.m.t`, a nested field: its loop runs am3
    // in place, the receiver the value of `this.m` at entry.
    // Wrapped in a function, as a CommonJS module is, so `DM` and `DV` are captures.
    let source = r#"(function () {
var DM = 0xfffffff, DV = 0x10000000;
function am3(i,x,w,j,c,n) {
  var this_array = this.array;
  var w_array    = w.array;
  var xl = x&0x3fff, xh = x>>14;
  while(--n >= 0) {
    var l = this_array[i]&0x3fff;
    var h = this_array[i++]>>14;
    var m = xh*l+h*xl;
    l = xl*l+((m&0x3fff)<<14)+w_array[j]+c;
    c = (l>>28)+(m>>14)+xh*h;
    w_array[j++] = l&0xfffffff;
  }
  return c;
}
function B(n, seed) { this.array = []; this.t = n; for (var k = 0; k < n; k++) this.array.push(((k + seed) * 2654435761) & 0xfffffff); }
B.prototype.am = am3;
function M(m) { this.m = m; this.mpl = 0x1234; this.mph = 0x56; this.um = 0x7fff; }
M.prototype.reduce = function reduce(x) {
  var x_array = x.array;
  for (var i = 0; i < this.m.t; ++i) {
    var j = x_array[i]&0x7fff;
    var u0 = (j*this.mpl+(((j*this.mph+(x_array[i]>>15)*this.mpl)&this.um)<<15))&DM;
    j = i+this.m.t;
    x_array[j] += this.m.am(0,u0,x,i,0,this.m.t);
    while(x_array[j] >= DV) { x_array[j] -= DV; x_array[++j]++; }
  }
  var s = 0;
  for (var k = 0; k < x_array.length; k++) s = (s * 31 + x_array[k]) | 0;
  return s;
};
var mod = new M(new B(30, 7)), t = 0;
for (var q = 0; q < 2000; q++) { var x = new B(61, q); x.array.push(0); t = (t + mod.reduce(x)) | 0; }
print(t);
})();
"#;
    let program = Engine::specialize(source, "osr-inline-field-receiver.js").unwrap();
    let function = |wanted: &str| {
        program
            .functions
            .iter()
            .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == wanted))
            .unwrap()
    };
    let (am3, reduce) = (function("am3"), function("reduce"));
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["-1925714172"]);
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        compiled.execute(&program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
        if !super::compiled::loop_tier_compiles(policy) {
            continue;
        }
        let views = compiled
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let osr = |function: usize| {
            views.get(function).and_then(super::program_store::derived_lane_view).expect("hot lane view").osr_diagnostics()
        };
        let (callee, caller) = (osr(am3), osr(reduce));
        assert!(caller.native_region_runs > 0, "no native caller run under {policy:?}: {caller:?}");
        // Out of line, am3's loop would run once per call, 2000 reductions of 30 calls; the
        // caller enters native code at its first backward jump, so a reduction's first
        // call still runs out of line.
        let calls = 2000 * 30;
        assert!(
            callee.native_region_runs * 40 < calls,
            "am3 ran out of line under {policy:?}: {callee:?} against {caller:?}"
        );
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_widens_an_int32_field_that_holds_a_double() {
    // `o.k` holds an `i32` while the loop compiles; once it holds 0.5 the entry declines,
    // naming the field, and the next compilation holds it as an `f64`.
    let source = r#"
      function sum(o, a) {
        var s = 0;
        for (var i = 0; i < o.n; i++) s += a[i] * o.k;
        return s;
      }
      var a = [], o = { n: 500, k: 3 }, t = 0;
      for (var i = 0; i < 500; i++) a.push(i);
      for (var c = 0; c < 400; c++) { if (c == 200) o.k = 0.5; t += sum(o, a); }
      print(t);
    "#;
    let program = Engine::specialize(source, "osr-field-widening.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "sum"))
        .unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["87325000"]);
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        compiled.execute(&program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
        if !super::compiled::loop_tier_compiles(policy) {
            continue;
        }
        let views = compiled
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let osr = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("hot sum loop lane view").osr_diagnostics();
        assert!(osr.entry_guard_misses > 0 && osr.compile_successes > 1, "the double field widened nothing under {policy:?}: {osr:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_drops_only_the_int32_guards_ranges_and_reads_cover() {
    // `m`'s products and sums stay in range and are read only by sums and bitwise
    // operators; `p` is read by a division, which tells -0 from +0, and `big` overflows.
    let source = r#"
function mix(a, k, n) {
  var s = 0, inf = 0, big = 0;
  for (var i = 0; i < n; i++) {
    var l = a[i] & 0x3fff, h = a[i] >> 14;
    var m = k * l + h * (k & 0x3fff);
    s = (s + ((m & 0x3fff) << 14) + (m >> 14)) | 0;
    var p = k * l;
    if (1 / p < 0 && p == 0) inf++;
    big = big + a[i] * 65536;
  }
  return s + " " + inf + " " + big;
}
var a = [];
for (var i = 0; i < 300; i++) a.push(i % 7 == 0 ? 0 : (i * 2654435761) & 0x7fffffff);
var out = "";
for (var c = 0; c < 300; c++) out = mix(a, c % 2 ? -5 : 3, 300);
print(out);
"#;
    let program = Engine::specialize(source, "osr-int32-facts.js").unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["-15100555 43 18079553300791296"]);
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.execute(&program).unwrap();
        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_wraps_sums_only_int32_conversions_read() {
    // `u` leaves the `i32`s but is read only by `|` and `>>`, which see it modulo 2^32;
    // `v`, the same sum, is compared, so it must hold the exact Number.
    let source = r#"
function wrap(a, n) {
  var s = 0, t = 0;
  for (var i = 0; i < n; i++) {
    var u = a[i] + 0x7ffffff0;
    s = (s + (u | 0) + (u >> 3)) | 0;
    var v = a[i] + 0x7ffffff0;
    if (v > 0x7fffffff) t++;
  }
  return s + " " + t;
}
var a = [];
for (var i = 0; i < 400; i++) a.push((i * 7) % 40);
var out = "";
for (var c = 0; c < 300; c++) out = wrap(a, 400);
print(out);
"#;
    let program = Engine::specialize(source, "osr-truncated-sums.js").unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.execute(&program).unwrap();
        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_counts_loops_whose_bodies_branch_forward() {
    // The loop's body clamps a double and branches on an element, and `continue` skips its
    // tail on some iterations: `pos` and `i` still step by one on every path, so the proof
    // covers the indexed accesses through them, while `odd`, which the paths step apart,
    // stays guarded.
    let source = r#"
function smear(d, d0, u, n, w, k) {
  var pos = 0, odd = 0;
  for (var i = 1; i <= n; i++) {
    var x = i - 0.37 * u[++pos];
    if (x < 0.5) x = 0.5; else if (x > w) x = w;
    var i0 = x | 0;
    if (u[pos] > 3) { odd++; d[pos] = d0[i0] * 0.5; } else d[pos] = d0[i0 + 1] - x;
    if (k > 0) continue;
    odd += 2;
  }
  return odd;
}
var n = 500, d = [], d0 = [], u = [];
for (var i = 0; i < n + 3; i++) { d.push(0); d0.push(i * 0.25); u.push((i * 7919) % 11 - 2.5); }
var t = 0;
for (var c = 0; c < 400; c++) t += smear(d, d0, u, n, n - 0.5, c % 3);
var s = 0;
for (var i = 0; i < d.length; i++) s += d[i];
print(t + " " + s.toFixed(6));
"#;
    let program = Engine::specialize(source, "osr-branching-counted-loop.js").unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.execute(&program).unwrap();
        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_bounds_an_index_sum_with_its_read() {
    // `j + k` feeds the element read right after it: one check below the length proves the
    // sum an `i32` too. A `k` that takes the sum past the `i32`s reads `undefined`, as the
    // general path does.
    let source = r#"
function gather(a, k, n) {
  var s = 0, misses = 0;
  for (var i = 0; i < n; i++) {
    var j = (i * 7) % n;
    var v = a[j + k];
    if (v === undefined) misses++; else s += v;
  }
  return s + ":" + misses;
}
var a = [];
for (var i = 0; i < 300; i++) a.push(i * 3 + 1);
var out = [];
for (var c = 0; c < 300; c++) out.push(gather(a, c % 50 == 49 ? 2147483600 : c % 3, 290));
print(out.slice(45, 52).join(" "));
"#;
    let program = Engine::specialize(source, "osr-index-sum.js").unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.execute(&program).unwrap();
        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_indexes_from_negative_carried_roots() {
    // Loop-carried `i32`s below zero key reads at constant offsets from them (`r + 2`,
    // `++r`): their views hold them sign-extended, so a key folds its offset into the
    // access from the root's own 64 bits.
    let source = r#"
function walk(a, start, n) {
  var s = 0, r = start, q = start + n + 2;
  for (var i = 0; i < n; i++) {
    s += a[r + 2] + a[q - 1];
    s += a[++r + 1];
    q--;
  }
  return s;
}
var a = [];
for (var i = 0; i < 64; i++) a.push(i * 5 + 3);
var out = [];
for (var c = 0; c < 300; c++) out.push(walk(a, -2 + (c % 3), 40));
print(out.slice(290).join(" "));
"#;
    let program = Engine::specialize(source, "osr-negative-roots.js").unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(crate::ExecutionMode::Compiled);
        compiled.execute(&program).unwrap();
        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_numeric_relations_keep_signed_int32_ordering() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function countNegative() {
            let index = -40;
            let count = 0;
            while (index < 0) {
              count++;
              index++;
            }
            return count;
          }
          let result = 0;
          for (let call = 0; call < 100; call++) result += countNegative();
          print(result);
        "#,
        "osr-signed-int32-relation.js",
    ))
    .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    assert_eq!(output.borrow().as_slice(), ["4000"]);
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "countNegative")
        })
        .unwrap();
    let views = vm
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot countNegative loop lane view");
    assert_eq!(view.osr_diagnostics().currently_compiled_sites, 1);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_numeric_truthiness_branches() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function zero() { return 0; }
          function whileTruthy(value) {
            let count = zero();
            while (value) {
              count++;
              if (!(value || 0)) value = 0;
              value--;
            }
            return count;
          }
          let total = 0;
          if (whileTruthy('x') !== 1) throw Error('non-number loop fallback');
          for (let call = 0; call < 300; call++) {
            total += whileTruthy(4);
          }
          print(total);
          if (whileTruthy(0) !== 0 || whileTruthy(-0) !== 0 || whileTruthy(NaN) !== 0) {
            throw Error('false numeric loop truthiness');
          }
          print(whileTruthy(2));
          print('ok');
        "#,
        "osr-numeric-truthiness.js",
    ))
    .unwrap();
    let _disassembly = program.disassemble();
    let function = program
        .functions
        .iter()
        .find(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "whileTruthy")
        })
        .expect("truthiness loop function");
    let function_id = program
        .functions
        .iter()
        .position(|candidate| std::ptr::eq(candidate, function))
        .expect("truthiness loop function index");
    let contains = |op| {
        function
            .code
            .iter()
            .any(|instruction| instruction.as_wide().op() == op)
    };
    assert!(
        contains(crate::bytecode::Op::JumpFalse),
        "truthiness loop must contain JumpFalse"
    );
    assert!(
        contains(crate::bytecode::Op::JumpTrue),
        "truthiness loop must contain JumpTrue"
    );
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = Vm::new(TestHost::writing_to(interpreted_output.clone()));
    interpreter.execute(&program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["1200", "2", "ok"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    assert_eq!(output.borrow().as_slice(), ["1200", "2", "ok"]);
    let views = vm
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot truthiness loop lane view");
    let diagnostics = view.osr_diagnostics();
    assert_eq!(
        diagnostics.currently_compiled_sites,
        1,
        "truthiness loop did not retain native code"
    );
    if std::env::var_os("QUENCH_JS_JIT_STATS").is_some() {
        assert!(diagnostics.compile_successes > 0, "loop OSR compilation did not succeed");
        assert!(diagnostics.native_region_runs > 0, "loop OSR code never ran");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_strict_null_identity_branches_on_rooted_values() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function seed() { return 0; }
          function countNull(value) {
            let count = seed();
            let hits = 0;
            let current = value;
            while (count < 4) {
              if (current === null) {
                hits++;
                current = value;
              } else {
                current = null;
              }
              count++;
            }
            return current === null ? hits + 10 : hits;
          }
          let result = 0;
          for (let call = 0; call < 400; call++) {
            result += countNull(call % 2 ? null : {});
          }
          print(result);
          print(countNull(undefined));
          print(countNull({}));
        "#,
        "osr-strict-null-identity.js",
    ))
    .unwrap();
    let _disassembly = program.disassemble();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "countNull")
        })
        .expect("strict null loop function");
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = Vm::new(TestHost::writing_to(interpreted_output.clone()));
    interpreter.execute(&program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["3200", "2", "2"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["3200", "2", "2"]);

    let views = vm
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot strict null loop lane view");
    let diagnostics = view.osr_diagnostics();
    assert_eq!(
        diagnostics.currently_compiled_sites,
        1,
        "OSR diagnostics: plain={}, admitted={}, attempts={}, successes={}, native={}, rejects={:?}",
        diagnostics.plain_backward_jump_sites,
        diagnostics.admitted_sites,
        diagnostics.compile_attempts,
        diagnostics.compile_successes,
        diagnostics.native_region_runs,
        diagnostics.analysis_rejections,
    );
    if std::env::var_os("QUENCH_JS_JIT_STATS").is_some() {
        assert!(
            diagnostics.native_region_runs > 0,
            "native OSR region never ran; diagnostics: {:?}",
            diagnostics.analysis_rejections
        );
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_strict_inequality_branches_for_null_and_undefined() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function count(value) {
            let index = 0;
            let total = 0;
            while (index < 4) {
              if (value !== null) total++;
              index++;
            }
            return total;
          }
          let nullTotal = 0;
          let undefinedTotal = 0;
          let objectTotal = 0;
          for (let call = 0; call < 300; call++) {
            nullTotal += count(null);
            undefinedTotal += count();
            objectTotal += count({});
          }
          print(nullTotal);
          print(undefinedTotal);
          print(objectTotal);
        "#,
        "osr-strict-inequality-nullish.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "count")
        })
        .expect("strict inequality loop function");

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone()))
        .execute(&program)
        .unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["0", "1200", "1200"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), interpreted_output.borrow().as_slice());

    let views = vm
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let diagnostics = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot strict inequality lane view")
        .osr_diagnostics();
    assert!(diagnostics.compile_successes > 0, "strict inequality loop did not compile: {diagnostics:?}");
    assert!(diagnostics.native_region_runs > 0, "strict inequality loop did not run natively: {diagnostics:?}");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_exits_at_effectful_calls_with_live_state_written_back() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          let calls = 0;
          let didThrow = false;
          const sentinel = {};
          function callback(index) {
            calls++;
            const allocated = { index, padding: [index, index + 1, index + 2] };
            if (index === 13 && !didThrow) {
              didThrow = true;
              throw sentinel;
            }
            return allocated.index;
          }
          globalThis.scale = 1;
          function run(callback) {
            let index = 0;
            let total = 0;
            while (index < 100) {
              total += index * scale;
              callback(index);
              index++;
            }
            return total;
          }
          let caught = 0;
          let total = 0;
          for (let invocation = 0; invocation < 50; invocation++) {
            try {
              total += run(callback);
            } catch (error) {
              if (error === sentinel) caught++;
              else throw error;
            }
          }
          print(caught);
          print(calls);
          print(total);
        "#,
        "osr-effectful-call-boundary.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .expect("hot callback loop function");
    let call_pc = program
        .function_get(function_id)
        .expect("hot callback loop bytecode")
        .instructions()
        .enumerate()
        .find_map(|(pc, instruction)| (instruction.op() == crate::bytecode::Op::Call).then_some(pc as u32))
        .expect("loop contains an ordinary callback call");

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["1", "4914", "242550"]);

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.enable_js_jit_diagnostics();
    compiled.execute(&program).unwrap();
    assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice());

    let views = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let diagnostics = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot callback loop lane view")
        .osr_diagnostics();
    assert!(diagnostics.compile_successes > 0, "call boundary did not admit a native prefix: {diagnostics:?}");
    assert!(diagnostics.native_region_runs > 0, "native prefix never ran: {diagnostics:?}");
    assert!(diagnostics.sites.iter().any(|site| {
        site.backedge_pc > site.header_pc
            && site.native_region_runs > 0
            && site.call_boundary_pcs.contains(&call_pc)
    }), "OSR diagnostics should report the native loop's call boundary at its exact PC: {diagnostics:?}");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reads_initialized_captured_environment_locals_and_deopts_non_numbers() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function run(initial) {
            let captured = initial;
            function readCaptured() { return captured; }
            let index = 0;
            let total = 0;
            while (index < 100) {
              total += index;
              total += captured;
              index++;
            }
            return total + readCaptured();
          }
          let total = 0;
          for (let invocation = 0; invocation < 50; invocation++) total += run(2);
          print(total);
          print(run("x"));
        "#,
        "osr-environment-local-boundary.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .expect("captured local loop function");
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted_output.clone())).execute(&program).unwrap();
    let expected = interpreted_output.borrow().clone();
    assert_eq!(expected.len(), 2);
    assert_eq!(expected[0], "257600");

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = Vm::new(TestHost::writing_to(compiled_output.clone()));
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.enable_js_jit_diagnostics();
    compiled.execute(&program).unwrap();
    assert_eq!(compiled_output.borrow().as_slice(), expected.as_slice());

    let views = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let diagnostics = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot captured local lane view")
        .osr_diagnostics();
    assert!(diagnostics.compile_successes > 0, "environment-local boundary was not compiled: {diagnostics:?}");
    assert!(diagnostics.native_region_runs > 0, "environment-local boundary never ran natively: {diagnostics:?}");
    assert!(diagnostics.exits > 0, "a non-Number local did not fall back at its load: {diagnostics:?}");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reloads_changing_field_receivers_and_exits_before_effects() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function sumList(node) {
            let total = 0;
            while (node !== null) {
              total += node.value;
              node = node.next;
            }
            return total;
          }
          let head = null;
          for (let value = 32; value > 0; value--) head = { pad: 0, marker: 0, value, next: head };
          globalThis.osrFieldHead = head;
          let repeated = 0;
          for (let call = 0; call < 300; call++) repeated += sumList(head);
          print(repeated);

          let getterCount = 0;
          const getterNode = { pad: 0, marker: 0, next: null };
          Object.defineProperty(getterNode, 'value', {
            get() { getterCount++; return 7; }, enumerable: true
          });
          print(sumList({ pad: 0, marker: 0, value: 1, next: getterNode }));
          print(getterCount);

          let proxyGets = 0;
          const proxyNode = new Proxy({ pad: 0, marker: 0, value: 9, next: null }, {
            get(target, key, receiver) {
              if (key === 'value' || key === 'next') proxyGets++;
              return Reflect.get(target, key, receiver);
            }
          });
          print(sumList({ pad: 0, marker: 0, value: 1, next: proxyNode }));
          print(proxyGets);

          const thrown = Error('field getter');
          let throwCount = 0;
          const throwingNode = { pad: 0, marker: 0, next: null };
          Object.defineProperty(throwingNode, 'value', {
            get() { throwCount++; throw thrown; }, enumerable: true
          });
          let sameError = false;
          try { sumList({ pad: 0, marker: 0, value: 1, next: throwingNode }); }
          catch (error) { sameError = error === thrown; }
          print(throwCount);
          print(sameError);
        "#,
        "osr-changing-field-receiver.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "sumList")
        })
        .expect("changing receiver loop function");

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = Vm::new(TestHost::writing_to(interpreted_output.clone()));
    interpreter.execute(&program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["158400", "8", "1", "10", "2", "1", "true"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), interpreted_output.borrow().as_slice());

    // A compiled raw field load embeds an object shape ID. Collection may compact and remap
    // those IDs, so the next loop admission must retire and rebuild the old region before
    // it can use the old slot descriptor.
    let native_runs_before_collection = {
        let views = vm
            .programs
            .lane_views(super::program_store::ProgramId::MAIN)
            .expect("main program lane views");
        let view = views
            .get(function_id)
            .and_then(super::program_store::derived_lane_view)
            .expect("hot changing receiver lane view");
        view.osr_diagnostics().native_region_runs
    };
    let shape_count_before_churn = vm.shapes.len();
    for index in 0..shape_count_before_churn * 2 {
        let atom = vm.intern_atom(&format!("dead-osr-field-{index}"));
        vm.transition_shape(0, atom);
    }
    let shape_count_before_collection = vm.shapes.len();
    let epoch_before_collection = crate::heap::prototype_layout_epoch();
    vm.collect_now(&program);
    assert!(crate::heap::prototype_layout_epoch() > epoch_before_collection);
    assert!(vm.shapes.len() < shape_count_before_collection, "collection did not compact the test shapes");

    let sum_list_atom = vm.intern_atom("sumList");
    let sum_list = vm.own_property(vm.realm.globals, sum_list_atom).expect("global sumList function");
    let head_atom = vm.intern_atom("osrFieldHead");
    let head = vm.own_property(vm.realm.globals, head_atom).expect("rooted linked-list head");
    assert_eq!(
        vm.call_value(&program, sum_list, Value::UNDEFINED, &[head]).unwrap(),
        Value::number(528.0)
    );

    let views = vm
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot changing receiver lane view after collection");
    let diagnostics = view.osr_diagnostics();
    assert_eq!(diagnostics.currently_compiled_sites, 1, "OSR diagnostics: {diagnostics:?}");
    assert!(
        diagnostics.compile_successes >= 2,
        "shape compaction did not recompile its stale field region: {diagnostics:?}"
    );
    assert!(
        diagnostics.native_region_runs > native_runs_before_collection,
        "changing receiver loop never entered native code: {diagnostics:?}"
    );
    assert_eq!(
        vm.programs
            .js_jit_field_cache_probes(super::program_store::ProgramId::MAIN, function_id as u32),
        Some(0),
        "the out-of-line changing-receiver loads still called the field probe"
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_loose_nullish_equality_without_coercion() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function seed() { return 0; }
          function countNonNull(value) {
            let count = seed();
            let hits = 0;
            while (count < 4) {
              if (value != null) hits++;
              count++;
            }
            return hits;
          }
          let coercions = 0;
          const object = { valueOf() { coercions++; throw Error('nullish equality coerced'); } };
          let result = 0;
          for (let call = 0; call < 400; call++) {
            result += countNonNull(call % 2 ? object : undefined);
          }
          if (coercions !== 0) throw Error('nullish equality coerced an object');
          print(result);
          print(countNonNull(null));
          print(countNonNull({}));
        "#,
        "osr-loose-nullish-equality.js",
    ))
    .unwrap();
    let _disassembly = program.disassemble();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "countNonNull")
        })
        .expect("loose nullish loop function");
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = Vm::new(TestHost::writing_to(interpreted_output.clone()));
    interpreter.execute(&program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["800", "0", "4"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["800", "0", "4"]);

    let views = vm
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot loose nullish loop lane view");
    let diagnostics = view.osr_diagnostics();
    assert_eq!(
        diagnostics.currently_compiled_sites,
        1,
        "OSR diagnostics: {diagnostics:?}"
    );
    assert!(
        diagnostics.native_region_runs > 0,
        "native OSR region never ran; diagnostics: {diagnostics:?}"
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_float_unary_uses_shared_number_denotation() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function negate(value) {
            let index = 0;
            while (index < 9999) {
              value = -value;
              index++;
            }
            return value;
          }
          for (let call = 0; call < 100; call++) negate(0.5);
          if (negate(0.5) !== -0.5) throw Error('float negation');
          if (!Object.is(negate(0), -0)) throw Error('negative zero');
          if (negate(-2147483648) !== 2147483648) throw Error('int32 minimum');
          print('ok');
        "#,
        "osr-shared-number-unary.js",
    ))
    .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    assert_eq!(output.borrow().as_slice(), ["ok"]);
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "negate")
        })
        .unwrap();
    let views = vm
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let view = views
        .get(function_id)
        .and_then(super::program_store::derived_lane_view)
        .expect("hot negate loop lane view");
    let osr = view.osr_diagnostics();
    assert_eq!(
        osr.admitted_sites, 1,
        "OSR analysis rejected the unary loop ({} rejection sites)",
        osr.analysis_rejections.len()
    );
    assert_eq!(
        osr.currently_compiled_sites, 1,
        "unary loop did not compile natively (admitted: {})",
        osr.admitted_sites
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_guarded_string_hash_loop_and_falls_back_after_intrinsic_change() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function quickHash(str) {
            let hash = 5381;
            let index = str.length;
            while (index > 0) {
              hash = hash * 33 ^ (str.charCodeAt(index) | 0);
              index -= 919;
            }
            return hash | 0;
          }
          if (quickHash('') !== 5381) throw Error('empty string');
          if (quickHash('abc') !== 177573) throw Error('short string');
          if (quickHash('😀') !== 177573) throw Error('surrogate pair');
          if (quickHash('a'.repeat(2000)) !== 193375941) throw Error('long string');
          String.prototype.charCodeAt = function() { return 99; };
          if (quickHash('abc') !== 177606) throw Error('intrinsic mutation fallback');
        "#,
        "js-jit-string-hash.js",
    ))
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(
        matches!(
            vm.programs
                .js_tier(super::program_store::ProgramId::MAIN, 1),
            Some(super::program_store::JsTierState::Compiled(_))
        ),
        "string hash loop should compile\n{}",
        program.disassemble()
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_leading_underscore_string_helpers_and_guards_intrinsic_changes() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function escapeLeadingUnderscores(value) {
            return value.length >= 2 && value.charCodeAt(0) === 95 && value.charCodeAt(1) === 95
              ? '_' + value : value;
          }
          function unescapeLeadingUnderscores(value) {
            const alias = value;
            return alias.length >= 3 && alias.charCodeAt(0) === 95
              && alias.charCodeAt(1) === 95 && alias.charCodeAt(2) === 95
              ? alias.substr(1) : alias;
          }
          for (let i = 0; i < 40; i++) {
            if (escapeLeadingUnderscores('__x') !== '___x') throw Error('escape match');
            if (escapeLeadingUnderscores('_x') !== '_x') throw Error('escape short prefix');
            if (escapeLeadingUnderscores('x') !== 'x') throw Error('escape miss');
            if (unescapeLeadingUnderscores('___x') !== '__x') throw Error('unescape match');
            if (unescapeLeadingUnderscores('__x') !== '__x') throw Error('unescape short prefix');
            if (unescapeLeadingUnderscores('x') !== 'x') throw Error('unescape miss');
          }
          String.prototype.charCodeAt = function() { return 95; };
          if (escapeLeadingUnderscores('ax') !== '_ax') throw Error('escape intrinsic fallback');
          if (unescapeLeadingUnderscores('abc') !== 'bc') throw Error('unescape intrinsic fallback');
        "#,
        "js-jit-leading-underscore.js",
    ))
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, 1),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
    assert!(
        matches!(
            vm.programs
                .js_tier(super::program_store::ProgramId::MAIN, 2),
            Some(super::program_store::JsTierState::Compiled(_))
        ),
        "unescape helper should compile\n{}",
        program.disassemble()
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_merges_boolean_branch_states_for_numeric_equality_chains() {
    let source =
        r#"
          function isStatementKindButNotDeclarationKind(kind) {
            return kind === 253 || kind === 252 || kind === 260 || kind === 247 ||
              kind === 245 || kind === 243 || kind === 250 || kind === 251 ||
              kind === 249 || kind === 246 || kind === 257 || kind === 254 ||
              kind === 256 || kind === 258 || kind === 259 || kind === 244 ||
              kind === 248 || kind === 255 || kind === 354;
          }
          function isFunctionLikeDeclarationKind(kind) {
            let switchValue = kind;
            switch (switchValue) {
              case 263: case 175: case 177: case 178:
              case 179: case 219: case 220: return true;
              default: return false;
            }
          }
          function logicalNotNumberConstant() { return !0; }
          const matches = [253, 252, 260, 247, 245, 243, 250, 251, 249, 246,
            257, 254, 256, 258, 259, 244, 248, 255, 354];
          const functionLikeMatches = [263, 175, 177, 178, 179, 219, 220];
          for (let kind = 0; kind < 360; kind++) {
            const expected = matches.includes(kind);
            const functionLikeExpected = functionLikeMatches.includes(kind);
            if (isStatementKindButNotDeclarationKind(kind) !== expected) {
              throw Error('equality chain ' + kind);
            }
            if (isFunctionLikeDeclarationKind(kind) !== functionLikeExpected) {
              throw Error('switch chain ' + kind);
            }
          }
          for (let i = 0; i < 40; i++) {
            if (logicalNotNumberConstant() !== true) throw Error('logical not constant');
          }
        "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        &source,
        "js-jit-boolean-joins.js",
    ))
    .unwrap();
    assert!(
        super::js_compiled::compile(&program, 1).is_some(),
        "equality chain should compile\n{}",
        program.disassemble()
    );
    assert!(
        super::js_compiled::compile(&program, 2).is_some(),
        "switch chain should compile\n{}",
        program.disassemble()
    );
    assert!(
        super::js_compiled::compile(&program, 3).is_some(),
        "constant logical not should compile; candidate={} function={:?}\n{}",
        super::js_compiled::function_candidate(program.function(3), &program),
        program.functions[3],
        program.disassemble()
    );
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, 1),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, 2),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, 3),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_cached_numeric_kind_field_switches() {
    let source = r#"
      function isMethodOrAccessor(node) {
        switch (node.kind) {
          case 175: case 178: case 179: return true;
          default: return false;
        }
      }
      function isBindingPattern(node) {
        if (node) {
          const kind = node.kind;
          return kind === 208 || kind === 207;
        }
        return false;
      }
      const kinds = [175, 178, 179, 180];
      for (let i = 0; i < 40; i++) {
        const kind = kinds[i % kinds.length];
        if (isMethodOrAccessor({kind}) !== (kind !== 180)) throw Error('kind switch');
        const bindingKind = i % 2 === 0 ? 208 : 207;
        if (isBindingPattern({kind: bindingKind}) !== true) throw Error('binding kind');
      }
      if (isBindingPattern(null) !== false) throw Error('falsey fallback');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        &source,
        "js-jit-kind-field.js",
    ))
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, 1),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, 2),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_lowers_guarded_numeric_field_reads_from_nonzero_formal() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function twice(scale, node) { return node.value * scale; }
          for (let index = 0; index < 40; index++) {
            if (twice(2, {value: index}) !== index * 2) throw Error('numeric field result');
          }
          let reads = 0;
          const accessor = { get value() { reads++; return 7; } };
          if (twice(2, accessor) !== 14 || reads !== 1) throw Error('accessor fallback');
          if (twice(2, {value: '7'}) !== 14) throw Error('cache replacement fallback');
        "#,
        "js-jit-guarded-field-arithmetic.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "twice")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function_id)
    else {
        panic!("numeric field arithmetic should compile");
    };
    assert_eq!(compiled.numeric_field_inputs().len(), 1);
    assert_eq!(compiled.params(), 2);
    assert_eq!(
        compiled.numeric_field_inputs()[0].receiver,
        super::js_compiled::JsJitFieldReceiver::Parameter(1)
    );
    assert!(compiled.native_entries() >= 9);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_lowers_guarded_numeric_field_reads_from_captured_objects() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function makeReader() {
            const state = {value: 7};
            function read() { return state.value * 2; }
            return {read, state};
          }
          const reader = makeReader();
          for (let index = 0; index < 40; index++) {
            if (reader.read() !== 14) throw Error('captured numeric field');
          }
          reader.state.value = '8';
          if (reader.read() !== 16) throw Error('captured field type guard');
          let reads = 0;
          Object.defineProperty(reader.state, 'value', {
            configurable: true,
            get() { reads++; return 9; }
          });
          if (reader.read() !== 18 || reads !== 1) throw Error('captured accessor fallback');
        "#,
        "js-jit-captured-object-field.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "read")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function_id)
    else {
        panic!("captured-object numeric field region should compile");
    };
    assert_eq!(compiled.numeric_field_inputs().len(), 1);
    assert!(matches!(
        compiled.numeric_field_inputs()[0].receiver,
        super::js_compiled::JsJitFieldReceiver::Capture { .. }
    ));
    assert!(compiled.native_entries() >= 9);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_lowers_multiple_guarded_this_field_reads_into_arithmetic() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function scaledSum(scale) { return this.left + this.right * scale; }
          const receiver = {left: 3, right: 4};
          for (let index = 0; index < 40; index++) {
            if (scaledSum.call(receiver, 2) !== 11) throw Error('this field arithmetic');
          }
          let reads = 0;
          const accessor = {get left() { reads++; return 3; }, right: 4};
          if (scaledSum.call(accessor, 2) !== 11 || reads !== 1) throw Error('this accessor fallback');
          const nonNumber = {left: '3', right: 4};
          if (scaledSum.call(nonNumber, 2) !== '38') throw Error('this non-number fallback');
        "#,
        "js-jit-guarded-this-fields.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "scaledSum")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function_id)
    else {
        panic!("multiple guarded `this` reads should compile");
    };
    assert_eq!(compiled.numeric_field_inputs().len(), 2);
    assert!(compiled.native_entries() >= 9);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_guarded_field_bitwise_boolean_regions() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function isHeldOrSuspended() {
            return (this.state & 1) !== 0 || this.state === 2;
          }
          for (let index = 0; index < 48; index++) {
            const state = index % 3;
            const receiver = {state};
            const expected = (state & 1) !== 0 || state === 2;
            if (isHeldOrSuspended.call(receiver) !== expected) throw Error('field bitwise result');
          }
          let reads = 0;
          const accessor = {get state() { reads++; return 1; }};
          if (isHeldOrSuspended.call(accessor) !== true || reads !== 1) throw Error('accessor fallback');
          if (isHeldOrSuspended.call({state: 1.5}) !== true) throw Error('fractional fallback');
          if (isHeldOrSuspended.call({state: '1'}) !== true) throw Error('coercion fallback');
        "#,
        "js-jit-field-bitwise-boolean.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "isHeldOrSuspended")
        })
        .unwrap() as u32;
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(compiled)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function_id)
    else {
        panic!("guarded field predicate should compile\n{}", program.disassemble());
    };
    assert_eq!(compiled.numeric_field_inputs().len(), 2);
    assert!(compiled.native_entries() >= 8);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_field_reject_resumes_interpreter_on_match_and_accessor() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function isFunctionBlock(node) { return node.isFunctionBlock === true; }
          function isBlockStatement(node) {
            if (242 !== node.tag) return !1;
            return !isFunctionBlock(node);
          }
          for (let index = 0; index < 40; index++) {
            const kind = index % 3 === 0 ? 242 : 241;
            if (isBlockStatement({tag: kind}) !== (kind === 242)) throw Error('field reject result');
          }
          let reads = 0;
          const accessor = { get tag() { reads++; return 242; } };
          if (isBlockStatement(accessor) !== true || reads !== 1) throw Error('accessor fallback');
          if (isBlockStatement({tag: '242'}) !== false) throw Error('non-number field fallback');
        "#,
        "js-jit-field-reject.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "isBlockStatement")
        })
        .unwrap() as u32;
    assert!(
        super::js_compiled::compile(&program, function_id).is_some(),
        "direct field-reject compile failed\n{}",
        program.disassemble()
    );
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(super::program_store::ProgramId::MAIN, function_id),
        Some(super::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_number_truthiness_branches() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function jumpFalse(value) {
            if (value) return 1;
            return 0;
          }
          function jumpTrue(value) {
            if (!value) return 2;
            return 3;
          }
          function conditional(value) {
            return value ? 4 : 5;
          }
          const values = [0, -0, NaN, 1, -1, Infinity, -Infinity, 'x'];
          for (const value of values) {
            const truthy = typeof value === 'number'
              ? value !== 0 && !Number.isNaN(value)
              : true;
            if (jumpFalse(value) !== (truthy ? 1 : 0)) throw Error('JumpFalse truthiness');
            if (jumpTrue(value) !== (truthy ? 3 : 2)) throw Error('JumpTrue truthiness');
            if (conditional(value) !== (truthy ? 4 : 5)) throw Error('conditional truthiness');
          }
          for (let index = 0; index < 40; index++) {
            jumpFalse(index - 20);
            jumpTrue(index - 20);
            conditional(index - 20);
          }
        "#,
        "js-jit-number-truthiness.js",
    ))
    .unwrap();
    let function_id = |name: &str| {
        program
            .functions
            .iter()
            .position(|function| {
                function
                    .header()
                    .name
                    .or(function.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32
    };
    let functions = ["jumpFalse", "jumpTrue", "conditional"].map(function_id);
    for function_id in functions {
        assert!(
            super::js_compiled::compile(&program, function_id).is_some(),
            "the numeric branch function should have a generic compiled denotation"
        );
    }

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).expect("interpreted truthiness");
    let mut compiled = Vm::new(TestHost::default());
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).expect("compiled truthiness");
    for function_id in functions {
        let Some(super::program_store::JsTierState::Compiled(artifact)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function_id)
        else {
            panic!("number truthiness function {function_id} was not compiled");
        };
        assert!(artifact.native_entries() > 0);
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_forward_numeric_branches_to_rooted_string_returns() {
    let source = r#"
      const rating = price => price < 100 ? '€' : price < 1000 ? '€€' : '€€€';
      for (let i = 0; i < 40; i++) {
        if (rating(99.99) !== '€') throw Error('below first boundary');
        if (rating(100) !== '€€') throw Error('first boundary');
        if (rating(999.99) !== '€€') throw Error('below second boundary');
        if (rating(1000) !== '€€€') throw Error('second boundary');
      }
      if (rating(-0) !== '€' || rating(-Infinity) !== '€') throw Error('negative number');
      if (rating(NaN) !== '€€€' || rating(Infinity) !== '€€€') throw Error('unordered/large number');

      let coercions = 0;
      const boxed = { valueOf() { coercions++; return 150; } };
      if (rating(boxed) !== '€€' || coercions !== 2) throw Error('guard fallback');

      function constructible(price) { return price < 100 ? '€' : '€€'; }
      for (let i = 0; i < 40; i++) constructible(i);
      const instance = new constructible(1);
      if (instance === '€' || !(instance instanceof constructible)) throw Error('constructor return');
    "#;
    let program = Engine::compile(crate::ExecutionRequest {
        source,
        name: "js-jit-string-branch.js",
        kind: crate::SourceKind::Script,
    })
    .unwrap();
    let function_count = program.functions.len();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();

    let compiled_string_functions: Vec<_> = (1..function_count)
        .filter_map(|function| {
            match vm
                .programs
                .js_tier(super::program_store::ProgramId::MAIN, function as u32)
            {
                Some(super::program_store::JsTierState::Compiled(compiled))
                    if compiled.has_string_returns() =>
                {
                    Some(compiled)
                }
                _ => None,
            }
        })
        .collect();
    assert!(
        !compiled_string_functions.is_empty(),
        "at least one forward-branch string function should reach native code"
    );
    assert!(compiled_string_functions
        .iter()
        .all(|compiled| compiled.native_entries() > 0));
}

#[test]
fn js_jit_tier_tracking_has_a_hard_bound() {
    let program = Engine::compile(crate::ExecutionRequest::script("void 0", "js-jit-cap.js"))
        .unwrap();
    let mut programs = super::program_store::ProgramStore::default();
    let id = programs.insert(program).unwrap();
    for function in 0..4096 {
        assert!(!programs.note_js_function_entry(id, function, false, false, false));
    }
    assert_eq!(programs.js_jit_tracked_function_count(), 2048);
    assert_eq!(programs.js_jit_tracking_exhaustion_count(), 2048);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_tier_tracks_hot_function_after_cold_function_churn() {
    let program = Engine::compile(crate::ExecutionRequest::script("void 0", "js-jit-cold-churn.js"))
        .unwrap();
    let mut programs = super::program_store::ProgramStore::default();
    programs.enable_js_general_dispatch();
    programs.compile_general_baseline_when_hot();
    let id = programs.insert(program).unwrap();

    for function in 0..512 {
        assert!(!programs.note_js_general_dispatch_entry(id, function, 1, crate::MemoryPolicy::Throughput));
    }
    let hot_function = 512;
    for _ in 0..31 {
        assert!(!programs.note_js_general_dispatch_entry(id, hot_function, 1, crate::MemoryPolicy::Throughput));
    }
    assert!(!programs.note_js_general_dispatch_entry(id, hot_function, 1, crate::MemoryPolicy::Throughput));
    assert!(programs.note_js_general_dispatch_entry(id, hot_function, 1, crate::MemoryPolicy::Throughput));

    assert_eq!(programs.js_jit_tracked_function_count(), 513);
    assert!(matches!(
        programs.js_tier(id, hot_function),
        Some(super::program_store::JsTierState::GeneralCompileDue)
    ));
    assert_eq!(programs.js_jit_tracking_exhaustion_count(), 0);
}

#[cfg(not(quench_profiled))]
#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_runs_dynamic_properties_with_shared_vm_semantics() {
    let source = r#"
      function read(target, key) {
        try { return target[key]; } catch (error) { return error; }
      }
      function write(target, key, value) {
        try { target[key] = value; return target[key]; } catch (error) { return error; }
      }
      function writeFixed(target, value) {
        try { target.fallback = value; return target.fallback; } catch (error) { return error; }
      }
      function strictWriteGetter(target) {
        'use strict';
        try { target.value = 99; return false; }
        catch (error) { return error instanceof TypeError; }
      }
      function readFixed(target) {
        try { return target.fallback; } catch (error) { return error; }
      }
      function readFixedGetter(target) {
        try { return target.value; } catch (error) { return error; }
      }
      let getterCalls = 0;
      const marker = {};
      const target = {
        get value() { getterCalls++; return getterCalls; },
        get throwing() { throw marker; },
        fallback: 0
      };
      for (let index = 1; index <= 48; index++) {
        if (read(target, 'value') !== index * 2 - 1) throw Error('getter result');
        if (write(target, 'fallback', index) !== index) throw Error('dynamic write result');
        if (readFixed(target) !== index) throw Error('cached field result');
        if (writeFixed(target, index + 100) !== index + 100) throw Error('cached own store result');
        if (readFixedGetter(target) !== index * 2) throw Error('cached getter result');
        if (!strictWriteGetter(target)) throw Error('strict getter-only store fallback');
      }
      if (getterCalls !== 96 || target.fallback !== 148) throw Error('dynamic property effects');
      if (read(target, 'throwing') !== marker) throw Error('caught getter throw');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(source, "js-jit-general.js"))
        .unwrap();
    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();

    let mut compiled = Vm::new(TestHost::default());

    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();

    for name in ["read", "write", "writeFixed", "readFixed", "readFixedGetter"] {
        let function = program
            .functions
            .iter()
            .position(|function| {
                function
                    .header()
                    .name
                    .or(function.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32;
        let Some(super::program_store::JsTierState::Compiled(native)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("{name} should use the general JavaScript dispatch compiler");
        };
        assert!(native.is_general_dispatch(), "{name} should not need a source shape");
        assert!(native.native_entries() > 0, "{name} should enter its native CFG");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_loads_rooted_string_and_bigint_constants_natively() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function literal(flag) {
            const value = flag
              ? 'shared constant'
              : 123456789012345678901234567890n;
            $262.gc();
            return value;
          }
          for (let index = 0; index < 48; index++) globalThis.result = literal(index % 2 === 0);
          if (literal(true) !== 'shared constant') throw Error('string constant');
          if (literal(false) !== 123456789012345678901234567890n) throw Error('bigint constant');
        "#,
        "js-jit-general-constants.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "literal")
        })
        .unwrap() as u32;
    let body = program.function(function as usize);
    let constant_pcs = body
        .code
        .iter()
        .enumerate()
        .filter_map(|(pc, packed)| {
            let instruction = super::dispatch_fast::decoded_instruction(body, *packed);
            // Only a LoadConst names a constant index.
            (instruction.op() == crate::bytecode::Op::LoadConst
                && matches!(
                    program.constants.get(instruction.constant_index()),
                    Some(
                        crate::bytecode::Constant::String(_)
                            | crate::bytecode::Constant::StringUnits(_)
                            | crate::bytecode::Constant::BigInt(_)
                    )
                ))
                .then_some(pc)
        })
        .collect::<Vec<_>>();
    assert!(
        constant_pcs.len() >= 2,
        "the function should load string and BigInt constants"
    );

    let mut interpreter = Vm::new(TestHost::default().with_test262());
    interpreter.execute(&program).unwrap();
    let result_name = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_name)
        .unwrap();
    let expected_text = interpreter.to_string(&program, expected).unwrap();

    let mut compiled = Vm::new(TestHost::default().with_test262());
    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();
    let result_name = compiled.intern_atom("result");
    let actual = compiled
        .own_property(compiled.realm.globals, result_name)
        .unwrap();
    assert_eq!(compiled.to_string(&program, actual).unwrap(), expected_text);

    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("literal should use the shared general JavaScript compiler");
    };
    assert!(native.is_general_dispatch());
    for pc in constant_pcs {
        assert!(
            native.general_dispatch_pc_is_native(pc),
            "heap-valued LoadConst at bytecode PC {pc} should be native"
        );
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_static_global_name_loads_keep_exact_fallbacks() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          globalThis.cachedData = 41;
          globalThis.nameGetterCalls = 0;
          Object.defineProperty(globalThis, 'accessorData', {
            configurable: true,
            get() { nameGetterCalls++; return 73; }
          });
          function strictTarget() { 'use strict'; return this === undefined; }
          globalThis.dynamicStrictTarget = strictTarget;

          function readCachedData() { return cachedData; }
          function callStrictTarget() { return dynamicStrictTarget(); }
          function readLexicalShadow() { return lexicalShadow; }
          function readTdz() { return tdzName; }
          function readAccessor() { return accessorData; }
          function readMissing() {
            try { return missingName; }
            catch (error) { return error instanceof ReferenceError; }
          }
          function typeofMissing() { return typeof missingName; }

          let lexicalShadow = 59;
          let sawTdz = true;
          for (let index = 0; index < 48; index++) {
            if (readCachedData() !== 41) throw Error('cached global data');
            if (!callStrictTarget()) throw Error('strict global call receiver');
            if (readLexicalShadow() !== 59) throw Error('global lexical shadow');
            try { readTdz(); sawTdz = false; }
            catch (error) { if (!(error instanceof ReferenceError)) throw error; }
            if (readAccessor() !== 73) throw Error('global accessor');
            if (!readMissing()) throw Error('missing global must throw');
            if (typeofMissing() !== 'undefined') throw Error('typeof missing global');
          }
          let tdzName = 61;
          if (!sawTdz) throw Error('global lexical TDZ');
          if (nameGetterCalls !== 48) throw Error('accessor count');
          globalThis.nameLoadResult = [
            readCachedData(), callStrictTarget(), readLexicalShadow(),
            readAccessor(), readMissing(), typeofMissing(), nameGetterCalls
          ];
        "#,
        "js-jit-general-static-names.js",
    ))
    .unwrap();

    let function_id = |name: &str| {
        program
            .functions
            .iter()
            .position(|body| {
                body.header()
                    .name
                    .or(body.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32
    };
    let name_sites = |function_id: u32, name: &str| {
        let body = program.function(function_id as usize);
        let atom = program
            .atoms
            .iter()
            .position(|candidate| candidate == name)
            .unwrap() as u32;
        body.code
            .iter()
            .enumerate()
            .filter_map(|(pc, packed)| {
                let instruction = super::dispatch_fast::decoded_instruction(body, *packed);
                // Only the name loads carry an atom index.
                (matches!(
                    instruction.op(),
                    crate::bytecode::Op::LoadName
                        | crate::bytecode::Op::LoadNameCall
                        | crate::bytecode::Op::LoadNameTypeof
                ) && instruction.atom_index() == atom)
                .then_some((pc, instruction.op()))
            })
            .collect::<Vec<_>>()
    };

    let eligible = [
        ("readCachedData", "cachedData", crate::bytecode::Op::LoadName),
        (
            "callStrictTarget",
            "dynamicStrictTarget",
            crate::bytecode::Op::LoadNameCall,
        ),
    ]
    .map(|(function, name, op)| (function, function_id(function), name, op));
    let fallback = [
        ("readLexicalShadow", "lexicalShadow", false),
        ("readTdz", "tdzName", false),
        ("readAccessor", "accessorData", true),
        ("readMissing", "missingName", true),
        ("typeofMissing", "missingName", true),
    ]
    .map(|(function, name, has_name_op)| (function, function_id(function), name, has_name_op));

    let mut interpreter = Vm::new(TestHost::default().with_test262());
    interpreter.execute(&program).unwrap();
    let result_atom = interpreter.intern_atom("nameLoadResult");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_atom)
        .unwrap();
    let expected = interpreter.to_string(&program, expected).unwrap();

    let mut compiled = Vm::new(TestHost::default().with_test262());
    compiled.programs.enable_js_general_dispatch();
    compiled.programs.compile_general_baseline_when_hot();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).unwrap();
    let result_atom = compiled.intern_atom("nameLoadResult");
    let actual = compiled
        .own_property(compiled.realm.globals, result_atom)
        .unwrap();
    assert_eq!(compiled.to_string(&program, actual).unwrap(), expected);

    for (name, function, atom, expected_op) in eligible {
        let Some(super::program_store::JsTierState::Compiled(native)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("{name} should use the general JavaScript compiler");
        };
        let pcs = name_sites(function, atom);
        assert_eq!(pcs.len(), 1, "{name} should have one global name site");
        assert_eq!(pcs[0].1, expected_op, "{name} should use the expected load op");
        assert!(
            native.general_dispatch_global_name_cache_hits() > 0,
            "eligible cached global load in {name} should take the cache-hit path"
        );
    }

    for (name, function, atom, has_name_op) in fallback {
        let Some(super::program_store::JsTierState::Compiled(native)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("{name} should use the general JavaScript compiler");
        };
        let pcs = name_sites(function, atom);
        assert_eq!(
            pcs.len(),
            usize::from(has_name_op),
            "{name} should have the expected number of unresolved name sites"
        );
        // Re-entering after a native call may safely cache unrelated names
        // such as `ReferenceError` in `readMissing`; the missing-name access
        // itself still runs through Vm::step and is covered by the parity case.
        if name != "readMissing" {
            assert_eq!(
                native.general_dispatch_global_name_cache_hits(),
                0,
                "lexical/effectful name load in {name} should retain exact fallback"
            );
        }
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_lowers_tagged_plain_local_accesses() {
    let source = r#"
      function transfer() { return this.invoke.apply(this, arguments); }
      const values = [{marker: 1}, undefined, NaN, 'tagged', true];
      const receiver = {invoke(value) { return value; }};
      let result;
      for (let index = 0; index < 64; index++) {
        result = transfer.call(receiver, values[index % values.length]);
      }
      if (result !== values[3]) throw Error('plain local tagged value changed');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-locals.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "transfer")
        })
        .unwrap();
    let body = program.function_get(function).unwrap();
    assert!(body
        .code
        .iter()
        .map(|packed| packed.as_wide().op().js_denotation())
        .any(|denotation| denotation == crate::bytecode::JsDenotation::PlainLocalLoad));
    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();

    let mut compiled = Vm::new(TestHost::default());

    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function as u32)
    else {
        panic!("transfer should use the shared general JavaScript compiler");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_lowers_return_to_the_shared_frame_dispatcher() {
    let source = r#"
      const shared = { identity: 1 };
      function choose(condition, first, second) {
        if (condition) return first;
        return second;
      }
      for (let index = 0; index < 64; index++) {
        const result = choose(index % 2 === 0, shared, shared);
        if (result !== shared) throw Error('return identity changed');
      }
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-return.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "choose")
        })
        .unwrap() as u32;
    let body = program.function_get(function as usize).unwrap();
    let return_pcs = body
        .code
        .iter()
        .enumerate()
        .filter_map(|(pc, packed)| {
            (super::dispatch_fast::decoded_instruction(body, *packed)
                .op()
                .js_denotation()
                == crate::bytecode::JsDenotation::Return)
                .then_some(pc)
        })
        .collect::<Vec<_>>();
    assert!(return_pcs.len() >= 2);

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("choose should use the shared general JavaScript compiler");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    for pc in return_pcs {
        assert!(
            native.general_dispatch_pc_is_native(pc),
            "Return at bytecode PC {pc} should complete through the shared frame dispatcher"
        );
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_speculates_number_arithmetic_and_falls_back_exactly() {
    let source = r#"
      function combine(left, right) {
        try { const result = left + right; return result; }
        catch (error) { return error instanceof TypeError; }
      }
      function negate(value) {
        try { return -value; }
        catch (error) { return error instanceof TypeError; }
      }
      function positive(value) {
        try { return +value; }
        catch (error) { return error instanceof TypeError; }
      }
      function multiply(left, right) {
        try { const result = left * right; return result; }
        catch (error) { return error instanceof TypeError; }
      }
      for (let index = 0; index < 48; index++) {
        if (combine(index, 0.5) !== index + 0.5) throw Error('number result');
        if (negate(index) !== -index) throw Error('unary minus');
        if (positive(index) !== index) throw Error('unary plus');
        if (multiply(index, 2) !== index * 2) throw Error('integer multiply');
      }
      if (combine(-2, -3) !== -5) throw Error('negative integer add');
      if (!Object.is(combine(-0, -0), -0)) throw Error('negative zero');
      if (combine(2147483647, 1) !== 2147483648) throw Error('integer boundary');
      if (!Object.is(multiply(0, -1), -0)) throw Error('integer multiply negative zero');
      if (multiply(50000, 50000) !== 2500000000) throw Error('integer multiply overflow');
      if (combine('left', 'right') !== 'leftright') throw Error('string fallback');
      if (combine(Symbol('value'), 1) !== true) throw Error('throw fallback');
      if (!Number.isNaN(combine(NaN, 1))) throw Error('NaN result');
      if (!Object.is(negate(0), -0) || !Object.is(negate(-0), 0)) {
        throw Error('unary zero signs');
      }
      if (negate(Symbol('value')) !== true || positive(Symbol('value')) !== true) {
        throw Error('unary throw fallback');
      }
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-arithmetic.js",
    ))
    .unwrap();
    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();

    let mut compiled = Vm::new(TestHost::default());

    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();

    for name in ["combine", "negate", "positive", "multiply"] {
        let function = program
            .functions
            .iter()
            .position(|function| {
                function
                    .header()
                    .name
                    .or(function.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32;
        let Some(super::program_store::JsTierState::Compiled(native)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("{name} should use the general JavaScript dispatch compiler");
        };
        assert!(native.is_general_dispatch());
        assert!(native.native_entries() > 0);
    }

    for (name, operator) in [
        ("combine", oxc_ast::ast::BinaryOperator::Addition as u32),
        (
            "multiply",
            oxc_ast::ast::BinaryOperator::Multiplication as u32,
        ),
    ] {
        let function = program
            .functions
            .iter()
            .position(|function| {
                function
                    .header()
                    .name
                    .or(function.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32;
        let body = program.function(function as usize);
        let native_pcs = body
            .code
            .iter()
            .enumerate()
            .filter_map(|(pc, packed)| {
                let instruction = super::dispatch_fast::decoded_instruction(body, *packed);
                (instruction.op().js_denotation()
                    == crate::bytecode::JsDenotation::NumericBinary
                    && instruction.binary_operator() == operator)
                    .then_some(pc)
            })
            .collect::<Vec<_>>();
        let Some(super::program_store::JsTierState::Compiled(native)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("{name} should have a general JavaScript artifact");
        };
        assert!(!native_pcs.is_empty());
        assert!(native_pcs
            .into_iter()
            .all(|pc| native.general_dispatch_pc_is_native(pc)));
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_speculates_number_relations_and_falls_back_exactly() {
    let source = r#"
      function identity(value) { return value; }
      function less(left, right) { return identity(left < right); }
      function strictEqual(left, right) { return identity(left === right); }
      function notEqual(left, right) { return identity(left !== right); }
      function looseEqual(left, right) { return identity(left == right); }
      for (let index = 0; index < 48; index++) {
        if (less(index, index + 1) !== true) throw Error('numeric less-than');
        if (strictEqual(index, index) !== true) throw Error('numeric strict equality');
        if (notEqual(index, index + 1) !== true) throw Error('numeric strict inequality');
        if (looseEqual(index, index) !== true) throw Error('numeric loose equality');
      }
      if (less(0, -0) || !strictEqual(0, -0)) throw Error('signed zero relations');
      if (less(NaN, 1) || less(1, NaN) || strictEqual(NaN, NaN)) {
        throw Error('unordered numeric relations');
      }
      if (!notEqual(NaN, NaN)) throw Error('NaN inequality');
      if (!less(-Infinity, -1) || !less(1.5, Infinity)) throw Error('float relation');
      if (!less('10', '2')) throw Error('string comparison fallback');

      let coercions = 0;
      const boxed = { valueOf() { coercions++; return 4; } };
      if (!less(boxed, 5) || coercions !== 1) throw Error('relational coercion fallback');
      if (!looseEqual(boxed, 4) || coercions !== 2) throw Error('loose equality coercion fallback');
      let symbolThrew = false;
      try { less(Symbol('value'), 1); }
      catch (error) { symbolThrew = error instanceof TypeError; }
      if (!symbolThrew) throw Error('symbol relational fallback');
      if (strictEqual(Symbol('value'), Symbol('value'))) throw Error('symbol equality');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-relations.js",
    ))
    .unwrap();
    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();

    for (name, operator) in [
        ("less", oxc_ast::ast::BinaryOperator::LessThan),
        ("strictEqual", oxc_ast::ast::BinaryOperator::StrictEquality),
        ("notEqual", oxc_ast::ast::BinaryOperator::StrictInequality),
        ("looseEqual", oxc_ast::ast::BinaryOperator::Equality),
    ] {
        let function = program
            .functions
            .iter()
            .position(|function| {
                function
                    .header()
                    .name
                    .or(function.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32;
        let Some(super::program_store::JsTierState::Compiled(native)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("{name} should use the general JavaScript compiler");
        };
        assert!(native.is_general_dispatch());
        assert!(native.native_entries() > 0);
        let body = program.function(function as usize);
        let relation_pcs = body
            .code
            .iter()
            .enumerate()
            .filter_map(|(pc, packed)| {
                let instruction = super::dispatch_fast::decoded_instruction(body, *packed);
                (instruction.op() == crate::bytecode::Op::Binary
                    && instruction.binary_operator() == operator as u32)
                    .then_some(pc)
            })
            .collect::<Vec<_>>();
        assert!(!relation_pcs.is_empty(), "{name} has no relation bytecode");
        assert!(relation_pcs
            .into_iter()
            .all(|pc| native.general_dispatch_pc_is_native(pc)),
            "{name} relation bytecode should have native code");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_reads_live_dense_array_elements_and_falls_back_exactly() {
    let source = r#"
      function readIndex(array, key) { return array[key]; }
      const first = [10, 20];
      const second = [30, 40];
      for (let index = 0; index < 48; index++) {
        const array = index % 2 === 0 ? first : second;
        if (readIndex(array, 0) !== (index % 2 === 0 ? 10 : 30)) throw Error('alternating receiver');
      }

      const copy = first.slice();
      copy[0] = 11;
      if (readIndex(copy, 0) !== 11 || readIndex(first, 0) !== 10) throw Error('copy-on-write backing');
      first[1] = 21;
      if (readIndex(first, 1) !== 21) throw Error('live backing value');

      let getterCalls = 0;
      const inherited = [1, 2];
      delete inherited[1];
      Object.setPrototypeOf(inherited, { get 1() { getterCalls++; return 50; } });
      if (readIndex(inherited, 1) !== 50 || getterCalls !== 1) throw Error('hole prototype getter');

      let proxyCalls = 0;
      const proxy = new Proxy([60], { get(target, key, receiver) {
        if (key === '0') proxyCalls++;
        return Reflect.get(target, key, receiver);
      } });
      if (readIndex(proxy, 0) !== 60 || proxyCalls !== 1) throw Error('proxy fallback');

      let coercions = 0;
      const key = { toString() { coercions++; return '0'; } };
      if (readIndex(second, key) !== 30 || coercions !== 1) throw Error('key coercion fallback');

      let accessorCalls = 0;
      const accessor = [];
      Object.defineProperty(accessor, 0, { get() { accessorCalls++; return 70; } });
      if (readIndex(accessor, 0) !== 70 || accessorCalls !== 1) throw Error('accessor fallback');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-dense-array-index.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "readIndex")
        })
        .unwrap() as u32;

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();

    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("readIndex should use the general JavaScript compiler");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    let body = program.function(function as usize);
    let get_index_pcs = body
        .code
        .iter()
        .enumerate()
        .filter_map(|(pc, packed)| {
            (super::dispatch_fast::decoded_instruction(body, *packed).op()
                == crate::bytecode::Op::GetIndex)
                .then_some(pc)
        })
        .collect::<Vec<_>>();
    assert_eq!(get_index_pcs.len(), 1);
    assert!(native.general_dispatch_pc_is_native(get_index_pcs[0]));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_batches_fallback_islands_before_native_ops() {
    let source = r#"
      const captured = { value: 2 };
      function addCaptured(value) {
        if (value === 63) $262.gc();
        return captured.value + value;
      }
      var result = 0;
      for (let index = 0; index < 64; index++) result = addCaptured(index);
      if (result !== 65) throw Error('interpreter island result');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-island.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "addCaptured")
        })
        .unwrap() as u32;

    let mut interpreter = Vm::new(TestHost::default().with_test262());
    interpreter.execute(&program).unwrap();
    let mut compiled = Vm::new(TestHost::default().with_test262());
    compiled.programs.enable_js_general_dispatch();
    compiled.programs.compile_general_baseline_when_hot();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).unwrap();

    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("addCaptured should use the general JavaScript compiler");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    assert!(native.general_dispatch_island_steps() > 0);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_reenters_callers_at_the_resume_pc() {
    let source = r#"
      function callee(value) {
        try { return value + 1; }
        catch (error) { return 0; }
      }
      function caller(value) { return callee(value) + 2; }
      var result = 0;
      for (let index = 0; index < 48; index++) result = caller(index);
      if (result !== 50) throw Error('call result');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-resume-pc.js",
    ))
    .unwrap();
    let caller = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "caller")
        })
        .unwrap() as u32;
    let callee = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "callee")
        })
        .unwrap() as u32;

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();
    let result_atom = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_atom)
        .unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, caller)
    else {
        panic!("caller should use the shared general JavaScript compiler");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    let Some(super::program_store::JsTierState::Compiled(native_callee)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, callee)
    else {
        panic!("ordinary calls from native code should tier their eligible callee");
    };
    assert!(native_callee.is_general_dispatch());
    assert!(native_callee.native_entries() > 0);
    let caller_body = program.function_get(caller as usize).unwrap();
    let (return_pc, return_register) = caller_body
        .code
        .iter()
        .enumerate()
        .find_map(|(pc, packed)| {
            let instruction = super::dispatch_fast::decoded_instruction(caller_body, *packed);
            (instruction.op() == crate::bytecode::Op::Return)
                .then_some((pc, instruction.register_a()))
        })
        .unwrap();
    let mut registers = vec![Value::UNDEFINED; usize::from(caller_body.registers)];
    registers[usize::from(return_register)] = Value::number(123.0);
    compiled.frames.push(Box::new(super::Frame {
        context: super::activation::CallContext::Internal,
        reflected_parameters: 0,
        original_arguments: Vec::new(),
        program: super::program_store::ProgramId::MAIN,
        function: caller,
        pc: return_pc,
        pc_is_resume: false,
        js_jit_skip_entry_once: false,
        source_field: None,
        binding_site_pc: None,
        env: Value::NULL,
        this: Value::UNDEFINED,
        locals: vec![Value::UNDEFINED; usize::from(caller_body.locals)],
        dynamic_bindings: Vec::new(),
        captured: false,
        initialized_registers: registers.len(),
        registers,
        capture_cache: super::CaptureCache::default(),
        active_iterators: Vec::new(),
        with_objects: Vec::new(),
        with_base: 0,
        js_jit_interpreter_started: None,
        fixed_this: false,
    }));
    let frame = compiled.frames.len() - 1;
    compiled.active_registers = compiled.frames[frame].registers.as_mut_ptr();
    let outcome = compiled
        .js_jit_general_dispatch_at_entry(
            &program,
            frame,
            return_pc,
            true,
            false,
        )
        .expect("the shared artifact accepts a nonzero bytecode entry PC");
    let super::compiled::JsNativeDispatchResult::Step(step) = outcome else {
        panic!("a Return entry should finish through the shared dispatcher");
    };
    assert_eq!(step.instruction_pc, return_pc);
    match step.result.unwrap() {
        super::StepResult::Return(value) => assert_eq!(value, Value::number(123.0)),
        _ => panic!("native Return should preserve its value"),
    }
    let result_atom = compiled.intern_atom("result");
    let result = compiled
        .own_property(compiled.realm.globals, result_atom)
        .unwrap();
    assert_eq!(result, expected);
    assert_eq!(result, Value::number(50.0));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_call_continuations_preserve_throw_and_gc_roots() {
    let source = r#"
      const marker = {};
      function callee(object, shouldThrow) {
        $262.gc();
        if (shouldThrow) throw marker;
        return object.value;
      }
      function caller(shouldThrow) {
        const live = { value: 40 };
        try { return callee(live, ...[shouldThrow]) + 2; }
        catch (error) { return error === marker ? live.value + 2 : -1; }
      }
      var result = 0;
      for (let index = 0; index < 48; index++) result = caller(index === 47);
      if (result !== 42) throw Error('call continuation lost result or live object');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-call-continuation.js",
    ))
    .unwrap();
    let function_id = |name: &str| {
        program
            .functions
            .iter()
            .position(|body| {
                body.header()
                    .name
                    .or(body.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32
    };
    let caller = function_id("caller");
    let callee = function_id("callee");

    let mut interpreter = Vm::new(TestHost::default().with_test262());
    interpreter.execute(&program).unwrap();
    let result_atom = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_atom)
        .unwrap();

    let mut compiled = Vm::new(TestHost::default().with_test262());
    compiled.programs.enable_js_general_dispatch();
    compiled.programs.enable_js_jit_diagnostics();
    compiled.programs.compile_general_baseline_when_hot();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).unwrap();
    let actual = compiled
        .own_property(compiled.realm.globals, result_atom)
        .unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual, Value::number(42.0));
    for function in [caller, callee] {
        let Some(super::program_store::JsTierState::Compiled(native)) = compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, function)
        else {
            panic!("call continuation function {function} should compile");
        };
        assert!(native.is_general_dispatch());
        assert!(native.native_entries() > 0);
    }
    let call_sites = compiled
        .programs
        .js_jit_call_site_counts(super::program_store::ProgramId::MAIN, caller)
        .expect("diagnostics should track call sites for the compiled caller");
    assert!(call_sites.iter().any(|(_, opcode, attempts, outcomes, direct_target, target_calls, ..)| {
        *opcode == crate::bytecode::Op::CallArray as u16
            && *attempts > 0
            && *direct_target == Some((super::program_store::ProgramId::MAIN.raw(), callee))
            && *target_calls == *attempts
            && outcomes[super::program_store::JsJitCallOutcome::PushFrame as usize] > 0
    }));
    let callee_call_sites = compiled
        .programs
        .js_jit_call_site_counts(super::program_store::ProgramId::MAIN, callee)
        .expect("diagnostics should track call sites for the compiled callee");
    assert!(callee_call_sites.iter().any(|(_, opcode, attempts, outcomes, _, _, _, _, sampled_calls, sampled_ns)| {
        *opcode == crate::bytecode::Op::Call as u16
            && *attempts > 0
            && outcomes[super::program_store::JsJitCallOutcome::Continue as usize] > 0
            && *sampled_calls > 0
            && *sampled_calls < *attempts
            && sampled_ns[super::program_store::JsJitCallOutcome::Continue as usize] > 0
    }));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_calls_compiled_numeric_callees_with_materialized_array_arguments() {
    let padding = "padding = padding + 1;\n".repeat(80);
    let source = format!(
        "function add(left, right) {{ return left + right; }}\
         function drive() {{ let padding = 0; {padding}\
         const result = add(1, ...[padding]); return result + 1; }}\
         var total = 0; for (let index = 0; index < 12000; index++) total = drive();\
         if (total !== 82) throw Error('array call lost its result');"
    );
    let program = Engine::compile(crate::ExecutionRequest::script(
        &source,
        "js-jit-call-array.js",
    ))
    .unwrap();
    let function_id = |name: &str| {
        program
            .functions
            .iter()
            .position(|body| {
                body.header()
                    .name
                    .or(body.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32
    };
    let add = function_id("add");
    let drive = function_id("drive");
    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();
    let total_atom = interpreter.intern_atom("total");
    let expected = interpreter
        .own_property(interpreter.realm.globals, total_atom)
        .unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.programs.enable_js_jit_diagnostics();
    compiled.programs.compile_general_baseline_when_hot();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).unwrap();
    let total_atom = compiled.intern_atom("total");
    assert_eq!(
        compiled
            .own_property(compiled.realm.globals, total_atom)
            .unwrap(),
        expected
    );
    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, add)
    else {
        panic!("the numeric array-call target should compile");
    };
    assert!(native.native_entries() >= 128);
    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, drive)
    else {
        panic!("the hot caller should compile after enough function entries");
    };
    assert!(native.is_general_dispatch());
    let call_sites = compiled
        .programs
        .js_jit_call_site_counts(super::program_store::ProgramId::MAIN, drive)
        .expect("the compiled caller should have call-site diagnostics");
    assert!(call_sites.iter().any(
        |(_, opcode, attempts, outcomes, target, target_calls, ..)| {
            *opcode == crate::bytecode::Op::CallArray as u16
                && *attempts > 0
                && *target == Some((super::program_store::ProgramId::MAIN.raw(), add))
                && *target_calls == *attempts
                && outcomes[super::program_store::JsJitCallOutcome::Continue as usize] > 0
        }
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_compiles_loop_backedges_and_resumes_vm_operations() {
    let source = r#"
      function sum(values) {
        let total = 0;
        let index = 0;
        do {
          try { total += values[index]; }
          catch (error) { total += 0; }
          index++;
        } while (index < values.length);
        return total;
      }
      var result = 0;
      for (let run = 0; run < 48; run++) result = sum([1, 2, 3, 4]);
      if (result !== 10) throw Error('loop result');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-loop.js",
    ))
    .unwrap();
    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();
    let name = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, name)
        .unwrap();

    let mut compiled = Vm::new(TestHost::default());

    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();

    let function = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or(function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "sum")
        })
        .unwrap() as u32;
    let body = program.function(function as usize);
    let backedges = body
        .code
        .iter()
        .enumerate()
        .filter_map(|(pc, packed)| {
            let instruction = super::dispatch_fast::decoded_instruction(body, *packed);
            (matches!(
                instruction.op(),
                crate::bytecode::Op::Jump
                    | crate::bytecode::Op::JumpFalse
                    | crate::bytecode::Op::JumpTrue
                    | crate::bytecode::Op::JumpBinaryFalse
            ) && instruction.result_flags() == 0
                && !instruction.returns_from_frame()
                && !instruction.writes_current_this()
                && usize::try_from(instruction.jump_target()).is_ok_and(|target| target <= pc))
            .then_some(pc)
        })
        .collect::<Vec<_>>();
    assert!(
        !backedges.is_empty(),
        "test bytecode should contain a loop latch"
    );
    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("loop function should use the general JavaScript compiler");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    for pc in backedges {
        assert!(
            native.general_dispatch_pc_is_native(pc),
            "loop backedge at bytecode PC {pc} should be native"
        );
    }
    let name = compiled.intern_atom("result");
    let result = compiled.own_property(compiled.realm.globals, name).unwrap();
    assert_eq!(result, expected);
    assert_eq!(result, Value::number(10.0));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_promotes_a_single_activation_hot_loop() {
    let source = r#"
      function sum(count) {
        let total = 0;
        for (let index = 0; index < count; index++) total += index;
        return total;
      }
      var result = sum(1000);
      if (result !== 499500) throw Error('loop result');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-single-activation-loop.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "sum")
        })
        .unwrap() as u32;
    let body = program.function_get(function as usize).unwrap();
    let backedge_pcs = body
        .code
        .iter()
        .enumerate()
        .filter_map(|(pc, packed)| {
            let instruction = super::dispatch_fast::decoded_instruction(body, *packed);
            (matches!(
                instruction.op(),
                crate::bytecode::Op::Jump
                    | crate::bytecode::Op::JumpFalse
                    | crate::bytecode::Op::JumpTrue
                    | crate::bytecode::Op::JumpBinaryFalse
            ) && usize::try_from(instruction.jump_target()).is_ok_and(|target| target <= pc))
            .then_some(pc)
        })
        .collect::<Vec<_>>();
    assert!(!backedge_pcs.is_empty());

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();
    let result_atom = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_atom)
        .unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.programs.compile_general_baseline_when_hot();
    compiled.execute(&program).unwrap();
    let tier = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function);
    let Some(super::program_store::JsTierState::Compiled(native)) = tier else {
        let reason = match tier {
            Some(super::program_store::JsTierState::Hot { observations, numeric }) => {
                format!("Hot(observations={observations}, numeric={numeric})")
            }
            Some(super::program_store::JsTierState::PromotionPending) => "PromotionPending".into(),
            Some(super::program_store::JsTierState::GeneralPromotionPending { entries }) => {
                format!("GeneralPromotionPending(entries={entries})")
            }
            Some(super::program_store::JsTierState::GeneralCompileDue) => "GeneralCompileDue".into(),
            Some(super::program_store::JsTierState::Rejected { entries, reason }) => {
                format!("Rejected(entries={entries}, reason={reason})")
            }
            Some(super::program_store::JsTierState::Unsupported) => "Unsupported".into(),
            Some(super::program_store::JsTierState::Compiled(_)) => "Compiled(other-tier)".into(),
            None => "untracked".into(),
        };
        panic!("a hot loop should promote its function into shared GeneralDispatch; tier={reason}");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    for pc in backedge_pcs {
        assert!(native.general_dispatch_pc_is_native(pc));
    }
    let result_atom = compiled.intern_atom("result");
    let result = compiled
        .own_property(compiled.realm.globals, result_atom)
        .unwrap();
    assert_eq!(result, expected);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_attributes_frameless_arrow_loop_work_to_its_owner() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function invoke() {
            let count = 1000;
            const countDown = () => {
              while (count > 0) count--;
              return count;
            };
            countDown(); count = 1000;
            countDown(); count = 1000;
            countDown(); count = 1000;
            countDown(); count = 1000;
            countDown(); count = 1000;
            countDown(); count = 1000;
            countDown(); count = 1000;
            return countDown();
          }
          var result = invoke();
          if (result !== 0) throw Error('frameless loop result');
        "#,
        "js-jit-frameless-arrow-loop-owner.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "countDown")
        })
        .unwrap() as u32;
    assert_eq!(program.function(function as usize).locals, 0);

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();
    let result_atom = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_atom)
        .unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.programs.compile_general_baseline_when_hot();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).unwrap();
    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("the frameless arrow's loop work should promote that function");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);

    let result_atom = compiled.intern_atom("result");
    assert_eq!(compiled.own_property(compiled.realm.globals, result_atom), Some(expected));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_prices_work_inside_compiled_osr_regions() {
    // Each taken backedge counts one unit of work. The first call's loop runs well past the
    // priced general-baseline floor for this small body (fixed plus per-instruction compile
    // cost, over the saved share), wherever its native runs end; the second call's entry
    // then compiles the due baseline. The handoff does not depend on the exact constants.
    let source = r#"
      function sum(count) {
        let total = 0;
        for (let index = 0; index < count; index++) total += index;
        return total;
      }
      var result = sum(1000000) + sum(1000000);
      if (result !== 999999000000) throw Error('OSR priced handoff result');
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(
        source,
        "js-jit-general-osr-priced-work.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "sum")
        })
        .unwrap() as u32;

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();
    let result_atom = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_atom)
        .unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).unwrap();
    let views = compiled
        .programs
        .lane_views(super::program_store::ProgramId::MAIN)
        .expect("main program lane views");
    let loop_view = views
        .get(function as usize)
        .and_then(super::program_store::derived_lane_view)
        .expect("sum function lane view");
    if std::env::var_os("QUENCH_JS_JIT_STATS").is_some() {
        assert!(
            loop_view.osr_diagnostics().native_region_runs > 0,
            "shared-tier promotion must be paid by native OSR work"
        );
    }
    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("paid native OSR work should hand the loop function to GeneralDispatch");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    let result = compiled
        .own_property(compiled.realm.globals, result_atom)
        .unwrap();
    assert_eq!(result, expected);
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_general_dispatch_attributes_inlined_osr_work_to_the_loop_owner() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        r#"
          function helper(limit) {
            let total = 0;
            for (let index = 0; index < limit; index++) total += index;
            return total;
          }
          function caller() { return helper(1000000); }
          var result = caller();
          if (result !== 499999500000) throw Error('inlined OSR result');
        "#,
        "js-jit-general-inline-osr-owner.js",
    ))
    .unwrap();
    let function_id = |name: &str| {
        program
            .functions
            .iter()
            .position(|body| {
                body.header()
                    .name
                    .or(body.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32
    };
    let helper = function_id("helper");
    let caller = function_id("caller");

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();
    let result_atom = interpreter.intern_atom("result");
    let expected = interpreter
        .own_property(interpreter.realm.globals, result_atom)
        .unwrap();

    let mut compiled = Vm::new(TestHost::default());
    compiled.programs.enable_js_general_dispatch();
    compiled.programs.enable_js_jit_diagnostics();
    compiled.programs.compile_general_baseline_when_hot();
    compiled.set_execution_mode(crate::ExecutionMode::Compiled);
    compiled.execute(&program).unwrap();
    let result_atom = compiled.intern_atom("result");
    assert_eq!(
        compiled
            .own_property(compiled.realm.globals, result_atom)
            .unwrap(),
        expected
    );

    let Some(super::program_store::JsTierState::Compiled(native)) = compiled
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, helper)
    else {
        panic!("OSR work from the inlined helper should promote the helper");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    assert!(!matches!(
        compiled
            .programs
            .js_tier(super::program_store::ProgramId::MAIN, caller),
        Some(super::program_store::JsTierState::Compiled(native))
            if native.is_general_dispatch()
    ));
}

#[cfg(not(quench_profiled))]
#[test]
fn js_jit_general_baseline_waits_until_entries_pay_for_its_compile() {
    let state = |calls: u32| {
        let source = format!(
            "function add(a, b) {{ return a + b; }} for (let i = 0; i < {calls}; i++) globalThis.last = add('s', i);"
        );
        let program = Engine::compile(crate::ExecutionRequest::script(&source, "js-jit-priced.js")).unwrap();
        let function = program
            .functions
            .iter()
            .position(|body| {
                body.header()
                    .name
                    .or(body.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == "add")
            })
            .unwrap() as u32;
        let mut interpreter = Vm::new(TestHost::default());
        interpreter.execute(&program).unwrap();
        let mut vm = Vm::new(TestHost::default());
        vm.programs.enable_js_general_dispatch();
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.execute(&program).unwrap();
        let name = interpreter.intern_atom("last");
        let expected = interpreter.own_property(interpreter.realm.globals, name).unwrap();
        let name = vm.intern_atom("last");
        assert_eq!(vm.own_property(vm.realm.globals, name).unwrap(), expected);
        vm.programs.js_tier(super::program_store::ProgramId::MAIN, function)
    };
    // This body is too small to pay for the general baseline through entries. It must not
    // enter GeneralPromotionPending, because that path has no backedges to accrue work.
    for calls in [48, 20_000] {
        assert!(!matches!(state(calls), Some(super::program_store::JsTierState::GeneralPromotionPending { .. })));
    }
}

#[cfg(not(quench_profiled))]
#[test]
fn js_jit_rejected_functions_promote_to_general_dispatch_and_keep_frames() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        "function add(a, b) { return a + b; } for (let i = 0; i < 48; i++) globalThis.last = add('s', i);",
        "js-jit-rejected-lane.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or(body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "add")
        })
        .unwrap() as u32;

    let mut interpreter = Vm::new(TestHost::default());
    interpreter.execute(&program).unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.programs.enable_js_general_dispatch();
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.compile_general_baseline_when_hot();
    vm.execute(&program).unwrap();

    let Some(super::program_store::JsTierState::Compiled(native)) = vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, function)
    else {
        panic!("the rejected numeric candidate should promote to the general JavaScript tier");
    };
    assert!(native.is_general_dispatch());
    assert!(native.native_entries() > 0);
    let name = vm.intern_atom("last");
    let expected = interpreter
        .own_property(interpreter.realm.globals, name)
        .unwrap();
    let actual = vm.own_property(vm.realm.globals, name).unwrap();
    assert_eq!(actual, expected);

    let mut exhausted = Vm::new(TestHost::default());

    exhausted.programs.enable_js_general_dispatch();
    exhausted.set_execution_mode(crate::ExecutionMode::Compiled);
    exhausted.programs.compile_general_baseline_when_hot();
    let exhausted_id = exhausted.programs.insert(program.clone()).unwrap();
    for _ in 0..32 {
        exhausted
            .programs
            .note_js_function_entry(exhausted_id, function, false, false, true);
    }
    exhausted
        .programs
        .install_js_tier(exhausted_id, function, None, false);
    assert!(
        !exhausted.js_tier_may_claim(&program, exhausted_id, function),
        "code-cache or codegen exhaustion remains terminal"
    );
}

#[test]
fn js_jit_promotion_requires_a_confident_numeric_profile() {
    let program = Engine::compile(crate::ExecutionRequest::script("void 0", "js-jit-profile-gate.js"))
        .unwrap();
    let mut programs = super::program_store::ProgramStore::default();
    let id = programs.insert(program).unwrap();
    for observation in 0..16 {
        assert!(!programs.note_js_function_entry(id, 1, observation < 15, false, true));
    }
    assert!(matches!(
        programs.js_tier(id, 1),
        Some(super::program_store::JsTierState::Hot {
            observations: 16,
            numeric: 15
        })
    ));
}

#[test]
fn js_jit_profile_gate_uses_prespecified_exact_binomial_looks() {
    use super::program_store::JS_JIT_PROMOTION_LOOKS;

    fn upper_tail(n: u8, k: u8, p: f64) -> f64 {
        let n = u32::from(n);
        let k = u32::from(k);
        let mut mass = (1.0 - p).powi(n as i32);
        let mut total = 0.0;
        for successes in 0..=n {
            if successes >= k {
                total += mass;
            }
            if successes < n {
                mass *= (n - successes) as f64 / f64::from(successes + 1) * p / (1.0 - p);
            }
        }
        total
    }

    // Each fixed look spends alpha/4 under p=0.8. Testing at these points
    // bounds false promotion across looks for this function at 5%.
    for (observations, minimum) in JS_JIT_PROMOTION_LOOKS {
        assert!(upper_tail(observations, minimum, 0.8) <= 0.05 / 4.0);
        assert!(upper_tail(observations, minimum - 1, 0.8) > 0.05 / 4.0);
    }

    let program = Engine::compile(crate::ExecutionRequest::script(
        "void 0",
        "js-jit-sequential-profile-gate.js",
    ))
    .unwrap();
    let mut programs = super::program_store::ProgramStore::default();
    let id = programs.insert(program).unwrap();
    for observation in 0..31 {
        assert!(!programs.note_js_function_entry(id, 1, observation != 0, false, true));
    }
    assert!(programs.note_js_function_entry(id, 1, true, false, true));
    assert!(matches!(
        programs.js_tier(id, 1),
        Some(super::program_store::JsTierState::Hot {
            observations: 32,
            numeric: 31
        })
    ));
}

#[cfg(not(quench_profiled))]
#[test]
fn js_jit_failed_numeric_look_promotes_general_dispatch_at_32_entries() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        "void 0",
        "js-jit-general-profile-gate.js",
    ))
    .unwrap();
    let mut programs = super::program_store::ProgramStore::default();
    let id = programs.insert(program).unwrap();
    for observation in 0..31 {
        assert!(
            !programs.note_js_function_entry(id, 1, false, false, true),
            "entry {observation} must not promote before the first profile look"
        );
    }
    assert!(!programs.note_js_function_entry(id, 1, false, false, true));
    assert!(matches!(
        programs.js_tier(id, 1),
        Some(super::program_store::JsTierState::GeneralPromotionPending { .. })
    ));
}

#[test]
fn js_jit_keeps_promotion_pending_until_a_numeric_entry() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        "void 0",
        "js-jit-pending-promotion.js",
    ))
    .unwrap();
    let mut programs = super::program_store::ProgramStore::default();
    let id = programs.insert(program).unwrap();
    for _ in 0..31 {
        assert!(!programs.note_js_function_entry(id, 1, true, false, true));
    }
    assert!(!programs.note_js_function_entry(id, 1, false, false, true));
    assert!(matches!(
        programs.js_tier(id, 1),
        Some(super::program_store::JsTierState::PromotionPending)
    ));
    assert!(programs.note_js_function_entry(id, 1, true, false, true));
}

#[cfg(quench_profiled)]
#[test]
fn js_jit_leaves_profiling_builds_on_the_instrumented_dispatcher() {
    let program = Engine::compile(crate::ExecutionRequest::script(
        "function add(a, b) { return a + b; } for (let i = 0; i < 40; i++) if (add(i, 2) !== i + 2) throw Error('bad result');",
        "js-jit-profile.js",
    ))
    .unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.execute(&program).unwrap();
    assert!(vm
        .programs
        .js_tier(super::program_store::ProgramId::MAIN, 1)
        .is_none());
}

#[test]
fn dynamic_function_canonical_source_and_wrapper_origins_match_in_both_modes() {
    let source = r#"
Error.prepareStackTrace=(_,frames)=>frames;
const fn=Function(' \n return new Error().stack[0];\n '),frame=fn();
if(frame.getLineNumber()!==4||frame.getColumnNumber()!==9)throw Error('body position');
if(fn.toString()!=='function anonymous(\n) {\n \n return new Error().stack[0];\n \n}')throw Error('body source');
const parameter=Function('p = new Error().stack[0]','return p;')();
if(parameter.getLineNumber()!==1||parameter.getColumnNumber()!==25)throw Error('parameter position');
if(Function('return typeof anonymous')()!=='undefined')throw Error('synthetic self binding');
if(Function('anonymous=17','return anonymous')()!==17)throw Error('parameter shadow');
let threw=false;try{Function('"use strict";return anonymous')()}catch(error){threw=error instanceof ReferenceError}
if(!threw)throw Error('strict synthetic self binding');
"#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "dynamic-function-source.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "dynamic-function-source.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn private_data_fields_keep_distinct_homes_and_duplicate_values_in_both_modes() {
    let source = r#"
class Base {constructor(object){return object}}
function make(){return class extends Base {#x;constructor(object,value){super(object);this.#x=value}read(){return this.#x}write(value){this.#x=value}evaluate(){return eval('this.#x')}}}
const A=make(),B=make();
for(const object of [Object.freeze({}),Object.seal({}),Object.preventExtensions({})]) {
 let rejected;
 try{new A(object,1)}catch(error){rejected=error instanceof TypeError}
 if(!rejected)throw Error('private field added to a non-extensible object');
}
for(const object of [{},[],function(){},Object.create(null)]) {
 new A(object,1);new B(object,2);
 if(A.prototype.read.call(object)!==1||B.prototype.read.call(object)!==2)throw Error('aliased class home');
 A.prototype.write.call(object,3);let duplicate;
 try{new A(object,4)}catch(error){duplicate=error.message}
 if(duplicate!=='Cannot initialize #x twice on the same object')throw Error('duplicate error');
 if(A.prototype.read.call(object)!==3||B.prototype.read.call(object)!==2)throw Error('duplicate overwrote data');
 if(A.prototype.evaluate.call(object)!==3||B.prototype.evaluate.call(object)!==2)throw Error('private eval home');
}
"#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "private-field-identity.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "private-field-identity.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
        let path = std::env::temp_dir().join(format!("quench-private-data-{}-{}", std::process::id(), program.specialized));
        program.write_binary(&path).unwrap();
        let decoded = crate::ResidualProgram::read_binary(&path).unwrap();
        Vm::new(TestHost::default()).execute(&decoded).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn eval_comment_only_sources_take_the_empty_script_fast_path() {
    let source = r#"
if (eval('/* block comment */ // trailing comment') !== undefined) throw Error('comment completion');
if (eval('\uFEFF/* Unicode space */\u2028// line comment') !== undefined) throw Error('Unicode trivia');
if (eval('// line comment\u2028 11') !== 11) throw Error('line separator ended comment');
if (eval('/* comment */ 17') !== 17) throw Error('executable source was skipped');
let invalid = false;
try { eval('/* unterminated'); } catch (error) { invalid = error instanceof SyntaxError; }
if (!invalid) throw Error('unterminated comment accepted');
"#;
    for program in [
        Engine::compile(crate::ExecutionRequest {
            source,
            name: "eval-comment-fast-path.js",
            kind: crate::SourceKind::Script,
        })
        .unwrap(),
        Engine::specialize(source, "eval-comment-fast-path.js").unwrap(),
    ] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn private_data_values_survive_collection_and_release_without_their_receiver() {
    let source = r#"
class Base {constructor(object){return object}}
function make(){return class extends Base {#x;constructor(object,value){super(object);this.#x=value}read(){return this.#x}}}
globalThis.A=make();globalThis.B=make();globalThis.first={value:1};globalThis.second={value:2};globalThis.instance={};
new A(instance,first);new B(instance,second);
"#;
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let program = compile(source, "private-data-roots.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&program).unwrap();
        let mut weak = Vec::new();
        for name in ["first", "second"] {
            let atom = vm.intern_atom(name);
            let value = vm.own_property(vm.realm.globals, atom).unwrap();
            let strong = vm.heap.root(value);
            weak.push(vm.embedding_host_weak_root(strong).unwrap());
            vm.heap.release_root(strong);
            vm.set_property(vm.realm.globals, atom, Value::UNDEFINED).unwrap();
        }
        vm.collect_now(&program);
        for root in &weak { assert!(vm.embedding_host_weak_value(*root).is_some()); }
        let instance = vm.intern_atom("instance");
        vm.set_property(vm.realm.globals, instance, Value::UNDEFINED).unwrap();
        vm.collect_now(&program);
        for root in weak {
            assert!(vm.embedding_host_weak_value(root).is_none());
            vm.heap.release_root(root);
        }
    }
}

#[test]
fn private_lexical_bindings_survive_ordinary_closures_and_binary_round_trips() {
    let source = r#"
class Mixed {
 #instance=1;static #static=2;
 read(){const object=this;return function(){return [eval('object.#instance'),eval('Mixed.#static')]}}
 static read(object){return eval('#instance in object')}
}
const object=new Mixed(), read=object.read();
if(read().join(',')!=='1,2'||!Mixed.read(object))throw Error('mixed private environment');
class Outer {#outer=3; create(){const outer=this;return class {#inner=4;read(){const inner=this;return function(){return [eval('outer.#outer'),eval('inner.#inner')]}}}}}
const Inner=new Outer().create();
if(new Inner().read()().join(',')!=='3,4')throw Error('outer private environment');
class Negative {#x;read(){return function(){for(const source of ['super.x','super()','return 1','#missing in {}']){let name;try{eval(source)}catch(error){name=error.name}if(name!=='SyntaxError')throw Error('accepted '+source)}}}}
new Negative().read()();
"#;
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let program = compile(source, "private-lexical-bindings.js").unwrap();
        Vm::new(TestHost::default()).execute(&program).unwrap();
        let path = std::env::temp_dir().join(format!("quench-private-lexical-{}-{}", std::process::id(), program.specialized));
        program.write_binary(&path).unwrap();
        let decoded = crate::ResidualProgram::read_binary(&path).unwrap();
        Vm::new(TestHost::default()).execute(&decoded).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn private_brands_follow_field_initialization_in_original_and_decoded_programs() {
    let source = r#"
class Instance {#field=1;#method(){}get #accessor(){return 2}static has(object){return [#field in object,#method in object,#accessor in object]}}
if(Instance.has(Instance.prototype).join(',')!=='false,false,false')throw Error('prototype branded');
if(Instance.has(new Instance()).join(',')!=='true,true,true')throw Error('instance missing brand');
class Static {static before=[#field in this,#method in this,#accessor in this];static #field=1;static #method(){}static get #accessor(){return 2}static after=[#field in this,#method in this,#accessor in this]}
if(Static.before.join(',')!=='false,true,true'||Static.after.join(',')!=='true,true,true')throw Error('static ordering');
class Base {constructor(object){return object}}
class Stamp extends Base {#x=7;static has(object){return #x in object}static read(object){return object.#x}}
if(Stamp.has(Stamp.prototype))throw Error('prototype already stamped');
new Stamp(Stamp.prototype);
if(!Stamp.has(Stamp.prototype)||Stamp.read(Stamp.prototype)!==7)throw Error('prototype stamp missing');
"#;
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let program = compile(source, "private-brand-initialization.js").unwrap();
        Vm::new(TestHost::default()).execute(&program).unwrap();
        let path = std::env::temp_dir().join(format!("quench-private-brands-{}-{}", std::process::id(), program.specialized));
        program.write_binary(&path).unwrap();
        let decoded = crate::ResidualProgram::read_binary(&path).unwrap();
        Vm::new(TestHost::default()).execute(&decoded).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn static_block_eval_inherits_strictness_and_its_actual_super_home() {
    let source = r#"
class Base {static value=7;static get receiver(){return this.name}static set update(value){this.updated=value}}
class Derived extends Base {
 static #value=3;
 static {
  if(eval('super.value')!==7||eval('super.receiver')!=='Derived'||eval('this.#value')!==3)throw Error('static context');
  if((()=>eval('super.value'))()!==7)throw Error('arrow home');
  let invalid;try{(function(){eval('super.value')})()}catch(error){invalid=error.name}
  if(invalid!=='SyntaxError')throw Error('ordinary super accepted');
  if(!eval('new.target === undefined'))throw Error('static new.target');
  for(const source of ['typeof arguments','(()=>typeof arguments)()']){let error;try{eval(source)}catch(caught){error=caught.name}if(error!=='SyntaxError')throw Error('static arguments accepted')}
  if(eval('(function(){return typeof arguments})()')!=='object')throw Error('ordinary arguments lost');
  eval('super.update=9');if(this.updated!==9)throw Error('setter receiver');
  let strict;try{eval('quenchStaticUnbound=1')}catch(error){strict=error.name}
  if(strict!=='ReferenceError')throw Error('sloppy static eval');
  Object.defineProperty(this,'locked',{value:1});let locked;try{this.locked=2}catch(error){locked=error.name}
  if(locked!=='TypeError')throw Error('sloppy static write');
  var local=11;if(eval('local')!==11)throw Error('local missing');
 }
 static {if(typeof local!=='undefined')throw Error('static scopes merged')}
}
"#;
    for compile in [Engine::specialize, Engine::specialize_unspecialized] {
        let program = compile(source, "static-block-context.js").unwrap();
        Vm::new(TestHost::default()).execute(&program).unwrap();
        let path = std::env::temp_dir().join(format!("quench-static-context-{}-{}", std::process::id(), program.specialized));
        program.write_binary(&path).unwrap();
        let decoded = crate::ResidualProgram::read_binary(&path).unwrap();
        Vm::new(TestHost::default()).execute(&decoded).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn spread_of_a_branch_built_array_survives_collection() {
    // The consequent's array reaches AppendArray only through the conditional's result
    // register, which must stay a register root while the spread allocates its iterator.
    let mut vm = Vm::new(TestHost::default());
    let script = crate::Engine::specialize(
        "var x = 5; var out = []; for (var i = 0; i < 2000; i++) { var a = [...(x ? [x, i] : [])]; out.push(a[0] + a[1]); } var total = out[1999] * 10000 + out.length;",
        "spread-branch-array.js",
    )
    .unwrap();
    vm.execute(&script).unwrap();
    let name = vm.intern_atom("total");
    let total = vm.own_property(vm.realm.globals, name).unwrap();
    assert_eq!(total.as_number(), Some(20_042_000.0));
}

#[test]
fn lexical_slots_read_only_after_initialization_become_plain_and_keep_their_dead_zone() {
    let source = r#"
      'use strict';
      const out = [];
      function t(f) { try { out.push(String(f())); } catch (e) { out.push(e.constructor.name + ':' + e.message); } }
      function proven() { let s = 0; for (let i = 0; i < 3; i++) { const y = i * 2; s += y; } return s; }
      t(proven);
      t(() => { const r = x; let x = 1; return r; });
      t(() => { switch (1) { case 0: let x = 1; break; case 1: return x; } });
      t(() => { const s = []; for (let i = 0; i < 2; i++) { if (i) s.push(z); let z = i; } return s; });
      t(() => { let r; try { r = w; } catch (e) { r = e.name; } let w = 3; return r + w; });
      t(() => { k = 2; let k; return k; });
      t(() => { let v; try { v = 1; } finally { v += 1; } return v; });
      t(() => { for (let x of [x]) {} });
      print(out.join('|'));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "lexical-initialization.js").unwrap();
    program.compile_released();
    let proven = program
        .functions
        .iter()
        .find(|function| function.name.is_some_and(|atom| program.atoms[atom as usize] == *"proven"))
        .unwrap();
    assert!(
        proven
            .instructions()
            .all(|instruction| !matches!(instruction.op(), crate::bytecode::Op::LoadLocal | crate::bytecode::Op::StoreLocal)),
        "every lexical slot of `proven` is read after its initialization"
    );
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["6|ReferenceError:Cannot access 'x' before initialization|\
          ReferenceError:Cannot access 'x' before initialization|\
          ReferenceError:Cannot access 'z' before initialization|ReferenceError3|\
          ReferenceError:Cannot access 'k' before initialization|2|\
          ReferenceError:Cannot access 'x' before initialization"]
    );
}

#[test]
fn anonymous_function_definitions_are_named_by_their_plan() {
    let source = r#"
      const arrow = () => 1;
      let expression = function () {};
      const { fallback = () => 2 } = {};
      const object = { method: () => 3, ['computed']: () => 4 };
      class Named { static field = () => 5; }
      const klass = class {};
      print([arrow.name, expression.name, fallback.name, object.method.name,
        object.computed.name, Named.field.name, klass.name].join(','));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "named-definitions.js").unwrap();
    program.compile_released();
    // Closures made straight into the named value are created with the name
    // (`inferred_name`); classes, computed keys and static field values are named at run time.
    for name in ["arrow", "expression", "fallback", "method"] {
        assert!(
            program.functions.iter().any(|function| {
                function.inferred_name.is_some_and(|atom| program.atoms[atom as usize] == *name)
            }),
            "{name} is named at closure creation"
        );
    }
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["arrow,expression,fallback,method,computed,field,klass"]
    );
}

#[test]
fn lane_closures_share_and_update_their_promoted_environment() {
    // Closures a lane makes promote their frame's environment once: siblings share it,
    // later writes are seen, and per-iteration bindings stay distinct.
    let source = r#"
      const out = [];
      function counter() { let n = 0; const inc = () => ++n; const get = () => n; return [inc, get]; }
      function outer(a) { let b = a * 2; function mid(c) { const f = () => a + b + c; b++; return f; } return mid; }
      for (let i = 0; i < 3; i++) {
        const [inc, get] = counter(); inc(); inc(); out.push(get());
        const m = outer(i); const f1 = m(1); const f2 = m(10); out.push(f1(), f2(), f1.name, typeof f2);
        const fs = []; for (let j = 0; j < 3; j++) fs.push(() => j); out.push(fs.map(f => f()).join(''));
        const o = {}; out.push(Object.keys(o).length, Object.getPrototypeOf(o) === Object.prototype);
      }
      print(out.join(','));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-closures.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["2,3,12,f,function,012,0,true,2,6,15,f,function,012,0,true,2,9,18,f,function,012,0,true"]
    );
}

#[test]
fn unary_operators_on_literals_fold_to_their_constants() {
    // `void` of a literal, `!` of a literal and the sign of a number fold at compile time;
    // signed zero, NaN, BigInt operands and string coercions keep their run-time results.
    let source = r#"
      const out = [];
      const z = -0, m = -(1), p = +2.5, n = -NaN;
      out.push(Object.is(z, -0), Object.is(-0, 0), m, p, Number.isNaN(n), 1 / -0);
      out.push(void 0, void "x", void (0), typeof void 0, void 0 === undefined);
      out.push(!0, !1, !"", !"a", !null, !(0), !NaN, !-0, !!1, !void 0);
      let x; out.push(x === void 0, x == void 0, x !== void 0);
      const o = { a: void 0, b: !0, c: !1 }; out.push(JSON.stringify(o), "a" in o, o.a);
      function f(a = void 0, b = !0) { return [a, b]; } out.push(f().join());
      out.push([void 0, !0, -1, +3].map(String).join());
      switch (!0) { case true: out.push("sw"); }
      out.push(-(-1), !!!0, !(!0), -"3", +"4", !1n, !0n, -(2 ** 2));
      out.push(typeof -0, (-0).toString(), 1 / (+(-0)));
      print(out.join("|"));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "unary-folding.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["true|false|-1|2.5|true|-Infinity||||undefined|true|true|false|true|false|true|true|true|true|true|true|true|true|false|{\"b\":true,\"c\":false}|true||,true|undefined,true,-1,3|sw|1|true|false|-3|4|false|true|-4|number|0|-Infinity"]
    );
}

#[test]
fn lane_char_code_at_keeps_its_coercions_and_patched_methods() {
    // `charCodeAt` of a string at an integer index runs in the lane; other receivers and
    // indexes, out-of-range positions and a replaced method keep their semantics.
    let source = r#"
      const out = [];
      function scan(s) { let t = 0; for (let i = -1; i <= s.length; i++) { const c = s.charCodeAt(i); t += Number.isNaN(c) ? 1000 : c; } return t; }
      for (let k = 0; k < 3; k++) {
        out.push(scan("abc"), scan(""), scan("é\u{1F600}"), "abc".charCodeAt(), "abc".charCodeAt(1.7), "abc".charCodeAt("2"), "abc".charCodeAt(NaN));
        out.push(new String("xyz").charCodeAt(1), String.prototype.charCodeAt.call(12, 0), "a".charCodeAt(2 ** 31));
        try { String.prototype.charCodeAt.call(null, 0); } catch (e) { out.push(e.constructor.name); }
        const o = { valueOf() { out.push("vo"); return 1; } }; out.push("abc".charCodeAt(o));
      }
      const saved = String.prototype.charCodeAt;
      String.prototype.charCodeAt = function (i) { return "patched" + i; };
      out.push("abc".charCodeAt(1));
      String.prototype.charCodeAt = saved;
      out.push("abc".charCodeAt(1));
      print(out.join(","));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-char-code-at.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["2294,2000,114422,97,98,99,97,121,49,NaN,TypeError,vo,98,2294,2000,114422,97,98,99,97,121,49,NaN,TypeError,vo,98,2294,2000,114422,97,98,99,97,121,49,NaN,TypeError,vo,98,patched1,98"]
    );
}

#[test]
fn constant_field_initializers_evaluate_in_the_constructor() {
    // Literal and literal-container initializers evaluate inline: order, signed zero, fresh
    // containers per instance, `__proto__` keys, setters on the prototype and derived classes.
    let source = r#"
      const out = [];
      let order = [];
      const key = (k) => (order.push(k), k);
      class Base { constructor() { out.push(Object.keys(this).join("+")); } }
      class A extends Base {
        n = -0; s = "str"; b = !0; u = void 0; arr = [1, "two", null]; obj = { a: 1, "b-c": 2, __proto__: null };
        [key("computed")] = 5; later = this.n; #p = 7; e = [];
        constructor(x) { super(); this.x = x; }
        get p() { return this.#p; }
      }
      class B { set s(v) { out.push("setter"); } }
      class C extends B { s = 1; }
      class D { y = 1; constructor(y) { out.push(this.y); } }
      for (let i = 0; i < 3; i++) {
        const a = new A(i), a2 = new A(i);
        out.push(Object.is(a.n, -0), a.s, a.b, a.u, a.arr.join("/"), a.arr !== a2.arr, a.e !== a2.e, Object.getPrototypeOf(a.obj) === null, a.obj["b-c"]);
        out.push(Object.keys(a).join(","), a.computed, Object.is(a.later, -0), a.p, Object.getOwnPropertyDescriptor(a, "s").writable);
        const c = new C(); out.push(c.s, Object.getOwnPropertyNames(c).join());
        out.push(new D(5).y);
      }
      out.push(order.join());
      print(out.join("|"));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "class-field-constants.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["||true|str|true||1/two/|true|true|true|2|n,s,b,u,arr,obj,computed,later,e,x|5|true|7|true|1|s|1|1|||true|str|true||1/two/|true|true|true|2|n,s,b,u,arr,obj,computed,later,e,x|5|true|7|true|1|s|1|1|||true|str|true||1/two/|true|true|true|2|n,s,b,u,arr,obj,computed,later,e,x|5|true|7|true|1|s|1|1|computed"]
    );
}

#[test]
fn function_field_initializers_keep_their_scope_and_activation() {
    // Arrow and function initializers evaluate inline unless the constructor binds a name
    // they read (their own parameters aside), or they read `new.target` or call `eval`.
    let source = r#"
      const out = [];
      const tag = "outer";
      class Base { hello() { return "base-hello"; } }
      class A extends Base {
        get = () => this.v;
        shadow = () => tag;
        nt = () => typeof new.target;
        ev = () => eval("typeof tag");
        sup = () => super.hello();
        fn = function () { return this === undefined ? "u" : typeof this; };
        named = () => 1;
        constructor(v) { super(); this.v = v; }
      }
      class B {
        shadow = () => tag;
        self = () => this;
        constructor(tag) { this.p = tag; }
      }
      class C { inc = () => ++this.n; n = 0; }
      const e = "outer-e", x = "outer-x";
      class M { own = (e, ...rest) => e + rest.length; reads = (y) => x + y; constructor(e, x) { this.e = e; } }
      for (let i = 0; i < 3; i++) {
        const a = new A(i), a2 = new A(i + 10);
        out.push(a.get(), a2.get(), a.shadow(), a.nt(), a.ev(), a.sup(), a.fn(), a.fn.call(5), a.named.name, a.get.name, a.get !== a2.get);
        const b = new B("ctor-param");
        out.push(b.shadow(), b.self() === b, b.p, Object.keys(b).join());
        const c = new C(); c.inc(); out.push(c.inc(), c.n);
        const m = new M("ctor-e", "ctor-x"); out.push(m.own("arg", 1, 2), m.reads("!"), m.e);
      }
      print(out.join("|"));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "class-field-functions.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["0|10|outer|undefined|string|base-hello|object|number|named|get|true|outer|true|ctor-param|shadow,self,p|2|2|arg2|outer-x!|ctor-e|1|11|outer|undefined|string|base-hello|object|number|named|get|true|outer|true|ctor-param|shadow,self,p|2|2|arg2|outer-x!|ctor-e|2|12|outer|undefined|string|base-hello|object|number|named|get|true|outer|true|ctor-param|shadow,self,p|2|2|arg2|outer-x!|ctor-e"]
    );
}

#[test]
fn lane_collection_methods_keep_receivers_keys_and_patched_methods() {
    // Map and Set methods run in the lane; `get`/`has` look up in place. SameValueZero keys,
    // subclasses, wrong receivers, mutation during a run and a replaced method stay exact.
    let source = r#"
      const out = [];
      const m = new Map([["a", 1], [NaN, "nan"], [0, "zero"], [{}, "obj"]]);
      const s = new Set(["x", -0, NaN]);
      const ws = new WeakSet(); const wm = new WeakMap(); const key = {}; ws.add(key); wm.set(key, 5);
      class M2 extends Map {}
      const m2 = new M2([["k", "v"]]);
      function run() {
        for (let i = 0; i < 3; i++) {
          out.push(m.get("a"), m.get(NaN), m.get(-0), m.get("missing"), m.get(), m.has("a"), m.has({}), s.has(0), s.has(NaN), s.has("y"), s.has());
          out.push(m2.get("k"), m2.has("k"), ws.has(key), wm.get(key));
          try { Map.prototype.get.call(s, "x"); } catch (e) { out.push(e.constructor.name); }
          try { Set.prototype.has.call(m, "a"); } catch (e) { out.push(e.constructor.name); }
          m.set("i" + i, i); out.push(m.get("i" + i), m.size); s.add(i); out.push(s.has(i), s.size);
          m.forEach((v, k) => { if (k === "a") out.push("fe" + v); });
          out.push([...m.keys()].length, [...s.values()].join("/"));
          m.delete("i" + i); s.delete(i);
        }
      }
      run();
      const saved = Map.prototype.get; Map.prototype.get = function (k) { return "patched:" + k; }; out.push(m.get("a")); Map.prototype.get = saved;
      print(out.join(","));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-collections.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["1,nan,zero,,,true,false,true,true,false,false,v,true,true,5,TypeError,TypeError,0,5,true,3,fe1,5,x/0/NaN,1,nan,zero,,,true,false,false,true,false,false,v,true,true,5,TypeError,TypeError,1,5,true,3,fe1,5,x/NaN/1,1,nan,zero,,,true,false,false,true,false,false,v,true,true,5,TypeError,TypeError,2,5,true,3,fe1,5,x/NaN/2,patched:a"]
    );
}

#[test]
fn ascii_string_building_keeps_text_across_the_inline_length() {
    // ASCII concatenation builds short results inline and longer ones in one buffer.
    let source = r#"
      const out = [];
      for (let n = 18; n <= 26; n++) {
        const a = "x".repeat(n - 4), s = `${a}${n}`;
        out.push(s.length, s.slice(-3), s === "x".repeat(n - 4) + String(n), (a + "é").length, `${a}é${n}`.charCodeAt(n - 4));
      }
      const parts = ["ab", "", "cd", "efghijklmnopqrstuvwxyz0123"]; let joined = "";
      for (const p of parts) joined = `${joined}${p}`;
      out.push(joined, joined.length, `${1}${2}${3}`, `${""}`, `${"a".repeat(22)}`.length, `${"a".repeat(23)}`.length);
      print(out.join("|"));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "string-building.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["16|x18|true|15|233|17|x19|true|16|233|18|x20|true|17|233|19|x21|true|18|233|20|x22|true|19|233|21|x23|true|20|233|22|x24|true|21|233|23|x25|true|22|233|24|x26|true|23|233|abcdefghijklmnopqrstuvwxyz0123|30|123||22|23"]
    );
}

#[test]
fn closure_names_keep_their_shapes_across_collections() {
    // Closures of one site take their named shape from a memo, which collections that
    // renumber shapes clear; names, descriptors and a later rename stay exact.
    let source = r#"
      const out = [];
      function make(i) {
        const named = () => i;
        const other = function inner() {};
        const method = { meth() {} }.meth;
        const anon = [() => 0][0];
        return [named, other, method, anon];
      }
      function churn() { let keep = null; for (let i = 0; i < 300000; i++) keep = { i, next: keep && i % 7 ? null : keep }; return keep; }
      for (let round = 0; round < 3; round++) {
        const fns = make(round);
        out.push(fns.map((f) => f.name).join("/"), Object.getOwnPropertyNames(fns[0]).join(), JSON.stringify(Object.getOwnPropertyDescriptor(fns[1], "name")));
        churn();
        const again = make(round + 10);
        out.push(again.map((f) => f.name).join("/"), again[0] !== fns[0]);
        Object.defineProperty(again[0], "name", { value: "renamed" });
        out.push(again[0].name, make(0)[0].name);
      }
      print(out.join("|"));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "closure-names.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["named/inner/meth/|length,name|{\"value\":\"inner\",\"writable\":false,\"enumerable\":false,\"configurable\":true}|named/inner/meth/|true|renamed|named|named/inner/meth/|length,name|{\"value\":\"inner\",\"writable\":false,\"enumerable\":false,\"configurable\":true}|named/inner/meth/|true|renamed|named|named/inner/meth/|length,name|{\"value\":\"inner\",\"writable\":false,\"enumerable\":false,\"configurable\":true}|named/inner/meth/|true|renamed|named"]
    );
}

#[test]
fn lane_in_place_steps_keep_iterator_closing_and_throws() {
    // Iterator steps run in place in a frame's lane run: break, return and throw close
    // iterators, next() throws and non-object results raise, destructuring and spread stay exact.
    let source = r#"
      const out = [];
      function userIter(n, log, opts = {}) {
        return { [Symbol.iterator]() { let i = 0; return { next() { if (opts.badResult && i === 1) return 5; if (opts.throwAt === i) throw new Error("next" + i); return { value: i, done: i++ >= n }; }, return() { log.push("ret"); return {}; } }; } };
      }
      function run(k) {
        const log = [];
        for (const v of [1, 2, 3]) { if (v === 2) break; log.push(v); }
        for (const v of userIter(3, log)) { if (v === 1) break; log.push("u" + v); }
        try { for (const v of userIter(3, log)) { if (v === 1) throw new Error("x"); } } catch (e) { log.push(e.message); }
        try { for (const v of userIter(3, log, { throwAt: 1 })) log.push("t" + v); } catch (e) { log.push(e.message); }
        try { for (const v of userIter(3, log, { badResult: true })) log.push("b" + v); } catch (e) { log.push(e.constructor.name); }
        const f = () => { for (const v of userIter(5, log)) { if (v === 2) return v; } };
        log.push(f());
        const [a, b, ...rest] = userIter(4, log); log.push(a, b, rest.join("+"));
        const [x] = userIter(4, log); log.push(x);
        log.push([...new Set([3, 1, 3])].join(), [..."héllo"].length, Array.from(new Map([[1, 2]])).join());
        function* gen() { try { yield 1; yield 2; } finally { log.push("fin"); } }
        for (const g of gen()) { log.push("g" + g); break; }
        try { for (const z of 5) {} } catch (e) { log.push(e.constructor.name); }
        class C { #p = k; static sum(list) { let s = 0; for (const c of list) s += #p in c ? c.#p : 0; return s; } }
        log.push(C.sum([new C(), {}, new C()]));
        out.push(log.join(","));
      }
      for (let k = 0; k < 3; k++) run(k);
      print(out.join("|"));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-iterators.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["1,u0,ret,ret,x,t0,next1,b0,TypeError,ret,2,0,1,2+3,ret,0,3,1,5,1,2,g1,fin,TypeError,0|1,u0,ret,ret,x,t0,next1,b0,TypeError,ret,2,0,1,2+3,ret,0,3,1,5,1,2,g1,fin,TypeError,2|1,u0,ret,ret,x,t0,next1,b0,TypeError,ret,2,0,1,2+3,ret,0,3,1,5,1,2,g1,fin,TypeError,4"]
    );
}

#[test]
fn lane_in_place_steps_keep_private_brands_and_field_definitions() {
    // Private fields, brand checks and class field definitions run in place; Proxy and
    // frozen receivers, double initialization and named field functions stay exact.
    let source = r#"
      const out = [];
      class Base { constructor(o) { if (o) return o; } }
      class Stamp extends Base { #s = 1; static has(o) { return #s in o; } static get(o) { return o.#s; } }
      class P { #a = 1; #b; #m() { return this.#a; } constructor(x) { this.#b = x; } read() { return [this.#a, this.#b, this.#m()].join(":"); } static isP(o) { try { o.#a; return true; } catch (e) { return e.constructor.name; } } }
      for (let i = 0; i < 3; i++) {
        out.push(new P(i).read(), P.isP(new P(1)), P.isP({}));
        const target = {}; new Stamp(target); out.push(Stamp.has(target), Stamp.get(target));
        try { new Stamp(target); } catch (e) { out.push(e.constructor.name); }
        const proxy = new Proxy({}, {}); new Stamp(proxy); out.push(Stamp.has(proxy));
        class F extends Base { x = 1; y = 2; }
        const frozen = Object.freeze({}); try { new F(frozen); } catch (e) { out.push(e.constructor.name); }
        const seen = []; const prox = new Proxy({}, { defineProperty(t, k, d) { seen.push(k); return Reflect.defineProperty(t, k, d); } });
        new F(prox); out.push(seen.join("+"), Object.keys(new F()).join());
        class N { f = function () {}; g = () => {}; ["h"] = class {}; } const n = new N(); out.push(n.f.name, n.g.name, n.h.name);
      }
      print(out.join("|"));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-private-fields.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["1:0:1|true|TypeError|true|1|TypeError|true|TypeError|x+y|x,y|f|g|h|1:1:1|true|TypeError|true|1|TypeError|true|TypeError|x+y|x,y|f|g|h|1:2:1|true|TypeError|true|1|TypeError|true|TypeError|x+y|x,y|f|g|h"]
    );
}

#[test]
fn lane_framed_calls_and_returns_keep_frames_and_their_semantics() {
    // Calls of functions that need a frame are pushed and returned from the lane: closures,
    // `arguments`, `this` modes, throws, recursion, tail returns, generators, async functions
    // and `eval` keep the general loop's results.
    let source = r#"
      const out = [];
      function counter() { let n = 0; return { inc: () => ++n, get: () => n }; }
      function withArgs() { return arguments.length + ":" + [].join.call(arguments, "-"); }
      function sloppyThis() { return typeof this; }
      function strictThis() { "use strict"; return this === undefined ? "u" : typeof this; }
      function thrower(x) { const f = () => x; if (x > 1) throw new Error("boom" + f()); return f(); }
      function deep(n) { const g = () => n; return n === 0 ? g() : deep(n - 1) + g(); }
      function tail(n) { const h = () => n; if (n === 0) return h(); return tail(n - 1); }
      class K { constructor() { this.v = 1; } }
      function callsCtor() { try { return K(); } catch (e) { return e.constructor.name; } }
      function* gen() { yield 1; }
      async function af() { return 2; }
      function usesEval(s) { const local = 5; return eval(s); }
      function run() {
        const c = counter(); c.inc(); c.inc();
        for (let i = 0; i < 4; i++) {
          out.push(c.inc(), c.get(), withArgs(i, "a"), sloppyThis(), strictThis(), deep(3), tail(4), callsCtor());
          try { out.push(thrower(i)); } catch (e) { out.push(e.message); }
          out.push([...gen()].join(), typeof af(), usesEval("local + 1"));
          const o = { m() { const x = () => this; return x() === o; } }; out.push(o.m());
          const err = (() => { try { null.x; } catch (e) { return e.constructor.name; } })(); out.push(err);
        }
      }
      run();
      print(out.join(","));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-framed-calls.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["3,3,2:0-a,object,u,6,0,TypeError,0,1,object,6,true,TypeError,4,4,2:1-a,object,u,6,0,TypeError,1,1,object,6,true,TypeError,5,5,2:2-a,object,u,6,0,TypeError,boom2,1,object,6,true,TypeError,6,6,2:3-a,object,u,6,0,TypeError,boom3,1,object,6,true,TypeError"]
    );
}

#[test]
fn quoted_keys_with_lone_surrogates_and_escapes_keep_their_semantics() {
    // A quoted key is a static name only when well-formed (`string::is_well_formed`): lone
    // surrogates, escapes, a quoted `__proto__` and quoted class method names as Node gives them.
    let source = r#"
const o = { "a": 1, "\uD800": 2, "b": 3, "__proto__": { p: 4 }, "x\ty": 5, "\uDC00z": 7 };
class C { "\uD800m"() { return 8; } "ok"() { return 9; } }
const c = new C();
print(JSON.stringify(Object.keys(o).map((k) => Array.from(k, (ch) => ch.charCodeAt(0)))) + "|" + o.p + "|" + o["\uD800"] + "|" + c["\uD800m"]() + "|" + c.ok() + "|" + Object.getOwnPropertyNames(C.prototype).map((k) => k.length).join());
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "quoted-keys.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), ["[[97],[55296],[98],[120,9,121],[56320,122]]|4|2|8|9|11,2,2"]);
}

#[test]
fn lane_settled_async_calls_keep_their_promises_and_job_order() {
    // Non-suspending async callees run as settling lane activations
    // (`Completion::SettlesPromise`): returns, throws, thenables, an impure `then` read
    // (settled in the materialized frame), nested and returned calls, native callbacks that
    // collect, and reaction order stay as the general loop's settled frames make them.
    let source = r#"
      const out = [];
      async function value(x) { return x; }
      async function thrower(x) { if (x & 1) throw new Error("boom" + x); return "ok"; }
      const thenableObject = { then(resolve) { resolve(70); } };
      async function thenable(x) { return thenableObject; }
      const tricky = { x: 5 };
      Object.defineProperty(tricky, "then", { get() { out.push("then" + out.length); return undefined; } });
      async function getterThen(x) { return tricky; }
      async function nested(x) { return value(x + 1); }
      async function collects(xs) { return xs.map((x) => { $262.gc(); return x * 2; }).join(""); }
      function returnsCall(x) { return value(x); }
      function drive(i) {
        return [value(i), thrower(i).catch((e) => e.message), thenable(i), getterThen(i).then((o) => o.x),
          nested(i), collects([i, i]), returnsCall(i)];
      }
      const order = [];
      const all = [];
      for (let i = 0; i < 40; i++) {
        all.push(...drive(i));
        value(i).then(() => order.push("a"));
        Promise.resolve().then(() => order.push("b"));
      }
      Promise.all(all).then((values) => { const text = values.join(","); let hash = 0; for (let k = 0; k < text.length; k++) hash = (hash * 31 + text.charCodeAt(k)) | 0; print(values.slice(0, 14).join(",") + "|" + values.length + ":" + hash + "|" + out.length + ":" + out.slice(-2).join(",") + "|" + order.join("").slice(0, 8)); });
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()).with_test262());
    vm.execute(&Engine::specialize(source, "lane-settled-async.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["0,ok,70,5,1,00,0,1,boom1,70,5,2,22,1|280:-1226768622|40:then38,then39|abababab"]
    );
}

#[test]
fn lane_arrows_take_this_from_their_environment() {
    // Arrows called as frameless activations read `this` from the environment they
    // closed over: call/apply/bind receivers are ignored, a derived constructor's
    // arrow throws before `super()` and sees the object after, and strict and
    // sloppy outer functions keep their own `this`.
    let source = r#"
      const out = [];
      const obj = { tag: 'o', m() { const a = () => this.tag; const b = (x) => x + (this ? this.tag : '-'); return [a(), b(1), a.call({ tag: 'X' }), b.apply({ tag: 'Y' }, [2])].join(); } };
      class P { constructor() { this.v = 'p'; } }
      class D extends P { constructor() { const early = () => { try { return this.v; } catch (e) { return e.constructor.name; } }; out.push(early()); super(); const late = () => this.v; out.push(late(), early()); } }
      function sloppyOuter() { const a = () => typeof this; return a(); }
      function strictOuter() { 'use strict'; const a = () => this; return a(); }
      function args() { const a = () => arguments.length; return a(); }
      const top = () => typeof this;
      for (let i = 0; i < 3; i++) {
        out.push(obj.m());
        new D();
        out.push(sloppyOuter(), String(strictOuter()), strictOuter.call(5), args(1, 2, 3), top());
        const bound = (() => this).bind({ z: 1 }); out.push(typeof bound());
        out.push([1, 2, 3].map((x) => x * 2).join(''));
      }
      print(out.join('|'));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-arrows.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["o,1o,o,2o|ReferenceError|p|p|object|undefined|5|3|object|object|246|o,1o,o,2o|ReferenceError|p|p|object|undefined|5|3|object|object|246|o,1o,o,2o|ReferenceError|p|p|object|undefined|5|3|object|object|246"]
    );
}

#[test]
fn bound_calls_and_base_class_constructions_pushed_in_place_keep_their_semantics() {
    // Bound calls and base class constructions run as in-loop frames: bound `this` and
    // argument order, nested binds, sloppy `this` coercion, `new.target`, fields, and a
    // constructor's primitive or object return all stay as a nested call makes them.
    let source = r#"
      'use strict';
      const out = [];
      function show(a, b, c) { return [this === undefined ? 'u' : typeof this === 'object' ? this.tag : String(this), a, b, c].join(':'); }
      const bound = show.bind({ tag: 'T' }, 1);
      const twice = bound.bind({ tag: 'ignored' }, 2);
      const loose = Function('return function(){ return this === globalThis ? "global" : typeof this; }')();
      for (let i = 0; i < 3; i++) {
        out.push(bound(i, 'x'), twice(i), show.bind(undefined)(i), loose.bind(null)(), loose.bind(5)());
      }
      class Base { x = 1; #p = 2; constructor(v) { this.v = v; this.nt = new.target === Base; } get p() { return this.#p; } }
      class Prim { constructor() { this.a = 1; return 7; } }
      class Obj { constructor() { return { other: true }; } }
      for (let i = 0; i < 2; i++) {
        const b = new Base(i);
        out.push([b.x, b.p, b.v, b.nt, Object.getPrototypeOf(b) === Base.prototype].join(','));
        out.push(new Prim().a, JSON.stringify(new Obj()));
        try { Base(1); } catch (e) { out.push(e.constructor.name); }
      }
      print(out.join('|'));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "bound-and-class-in-place.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["T:1:0:x|T:1:2:0|u:0::|global|object|T:1:1:x|T:1:2:1|u:1::|global|object|T:1:2:x|T:1:2:2|u:2::|global|object|1,2,0,true,true|1|{\"other\":true}|TypeError|1,2,1,true,true|1|{\"other\":true}|TypeError"]
    );
}

#[test]
fn lane_forwarded_calls_run_callees_of_other_programs_in_their_program() {
    // `f.call(...)` and a bound call of a callee compiled as another program, made from a
    // lane function often enough that its cross-program target is cached: the callee reads
    // its own program's names, constants and field sites (`globalThis`, a defined property,
    // string keys), not the caller's.
    let source = r#"
      const loose = Function('return function(){ return this === globalThis ? "global" : typeof this; }')();
      const definer = Function('o', 'k', 'Object.defineProperty(o, k, { value: k.length, enumerable: true }); o[k + "x"] = "v"; return Object.keys(o).join("+");');
      function run(i) {
        const bound = loose.bind(null);
        return [bound(), loose.call(null), loose.call(i), definer.call(null, {}, 'key' + i)].join(",");
      }
      const out = [];
      for (let i = 0; i < 24; i++) out.push(run(i % 2));
      print(out.join("|"));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-cross-program-forwarded.js").unwrap()).unwrap();
    let expected = (0..24)
        .map(|i| format!("global,global,object,key{0}+key{0}x", i % 2))
        .collect::<Vec<_>>()
        .join("|");
    assert_eq!(output.borrow().as_slice(), [expected]);
}

#[test]
fn lane_regexp_exec_keeps_last_index_and_coercion_order() {
    // Exec calls made from frameless activations: a global pattern advances `lastIndex`,
    // an object `lastIndex` is still converted exactly once per call, and a frozen global
    // pattern still throws when exec writes `lastIndex`.
    let source = r#"
      function step(re, text) { return re.exec(text); }
      function match(layer, text) { return step(layer.re, text); }
      const global = { re: /a/g };
      const seen = [];
      for (let i = 0; i < 3; i++) {
        const found = match(global, 'aab');
        seen.push(found ? found.index : 'null', global.re.lastIndex);
      }
      let conversions = 0;
      const plain = { re: /b/ };
      plain.re.lastIndex = { valueOf() { conversions++; return 0; } };
      for (let i = 0; i < 2; i++) seen.push(match(plain, 'ab').index);
      seen.push(conversions);
      const frozen = { re: Object.freeze(/a/g) };
      try { match(frozen, 'a'); seen.push('no throw'); } catch (error) { seen.push(error.constructor.name); }
      print(seen.join(','));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-regexp-exec.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), ["0,1,1,2,null,0,1,1,2,TypeError"]);
}

#[test]
fn catch_bindings_live_in_registers_and_keep_their_thrown_values() {
    // A catch binding read only by its block moves into a register: the wrapper calls no
    // frame of its own, and each catch still sees the value its own throw made.
    let source = r#"
      function wrap(layer, path) { try { return layer.match(path); } catch (err) { return err; } }
      const ok = { match(path) { return 'ok:' + path; } };
      const bad = { match(path) { throw new Error('bad:' + path); } };
      const seen = [];
      for (let i = 0; i < 3; i++) {
        seen.push(wrap(ok, i));
        seen.push(wrap(bad, i).message);
      }
      print(seen.join(','));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "catch-register.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["ok:0,bad:0,ok:1,bad:1,ok:2,bad:2"]);
    let wrap = program.function(1);
    assert!(wrap.handlers.iter().all(|handler| matches!(
        handler.slot,
        Some(crate::bytecode::ThrownValueSlot::Register(_))
    )));
    assert!(wrap.call_plan(&program).registers_only);
}

#[test]
fn promoted_var_locals_read_undefined_before_their_first_store() {
    // A frame's registers start stale from its pool, so a `var` promoted to a register that
    // a read reaches before any store must still read undefined on every call.
    let source = r#"
      function H(m) { this.message = m; }
      function f() { var err; var msg; msg = arguments[1]; if (!err) { err = new H(msg); } return err; }
      const first = f(404, 'config');
      const second = f(404, 'again');
      print([first.message, second.message, first !== second].join(','));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "promoted-var-entry.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), ["config,again,true"]);
}

#[test]
fn symbol_keyed_reads_follow_shapes_prototypes_and_redefinitions() {
    // Symbol-keyed reads answered from proofs (`symbol_load_proofs`) see deletes, shadows,
    // prototype changes, accessors, proxies, many shapes and collections as the walk does.
    let source = r#"
const k = Symbol('k'), m = Symbol('m');
const out = [];
function read(o, key) { let r; for (let i = 0; i < 4; i++) r = o[key]; return r; }
// own data, then delete and re-add
const a = { [k]: 1 };
out.push(read(a, k)); delete a[k]; out.push(read(a, k)); a[k] = 2; out.push(read(a, k));
// inherited through two prototypes, then a holder change, a shadow, a delete
const base = { [k]: 'base' }; const mid = Object.create(base); const obj = Object.create(mid);
out.push(read(obj, k)); base[k] = 'base2'; out.push(read(obj, k));
mid[k] = 'mid'; out.push(read(obj, k)); delete mid[k]; out.push(read(obj, k));
// absent, then added on a prototype
const p2 = {}; const o2 = Object.create(p2);
out.push(String(read(o2, m))); p2[m] = 'added'; out.push(read(o2, m));
Object.prototype[m] = 'top'; const o3 = {}; out.push(read(o3, m)); delete Object.prototype[m]; out.push(String(read(o3, m)));
// holder turned accessor
const h = { [k]: 'data' }; const o4 = Object.create(h); out.push(read(o4, k));
let calls = 0; Object.defineProperty(h, k, { get() { calls++; return this === o4 ? 'getter-this' : 'bad'; }, configurable: true });
out.push(read(o4, k), calls);
// receiver's prototype replaced
const pa = { [k]: 'pa' }, pb = { [k]: 'pb' }; const o5 = Object.create(pa); out.push(read(o5, k));
Object.setPrototypeOf(o5, pb); out.push(read(o5, k));
// two receivers of one shape with different prototypes
const q1 = Object.create(pa), q2 = Object.create(pb); out.push(read(q1, k), read(q2, k), read(q1, k));
// own accessor and non-writable own
const o6 = { get [k]() { return 'own-get'; } }; out.push(read(o6, k));
const o7 = {}; Object.defineProperty(o7, k, { value: 'ro', writable: false }); out.push(read(o7, k));
// class instance fields and prototype methods
class C { [k] = 'field'; [m]() { return 'method'; } }
class D extends C {}
const d = new D(); out.push(read(d, k), read(d, m).call(d));
C.prototype[m] = function () { return 'replaced'; }; out.push(read(d, m).call(d));
// symbol on a function, an array and a primitive wrapper
function f() {} f[k] = 'fn'; out.push(read(f, k));
const arr = [1]; arr[k] = 'arr'; out.push(read(arr, k), typeof read(arr, Symbol.iterator));
out.push(typeof read('s', Symbol.iterator), read(Object('s'), Symbol.toStringTag) === undefined);
// Proxy on the chain
const px = new Proxy({}, { get(t, key, r) { return key === k ? 'proxy' : undefined; } });
const o8 = Object.create(px); out.push(read(o8, k));
// many shapes through one site
const many = []; for (let i = 0; i < 40; i++) { const o = { ['x' + i]: i, [k]: i }; many.push(o); }
let sum = 0; for (let j = 0; j < 3; j++) for (const o of many) sum += o[k]; out.push(sum);
// collection in between
const keep = { [k]: 'kept' }; out.push(read(keep, k)); for (let i = 0; i < 200000; i++) ({ i }); out.push(read(keep, k));
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "symbol-loads.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"[1,null,2,"base","base2","mid","base2","undefined","added","top","undefined","data","getter-this",4,"pa","pb","pa","pb","pa","own-get","ro","field","method","replaced","fn","arr","function","function",true,"proxy",2340,"kept","kept"]"#]);
}

#[test]
fn inherited_getters_run_in_loop_and_follow_redefinitions() {
    // Getters found prototypes above the receiver, by name or symbol, run as in-loop frames
    // with the receiver as `this`; throws, shadowing, redefinition and deep chains keep the
    // results of the ordinary [[Get]].
    let source = r#"
const k = Symbol('k');
const out = [];
function read(o) { let r; for (let i = 0; i < 4; i++) r = o.v; return r; }
function readK(o) { let r; for (let i = 0; i < 4; i++) r = o[k]; return r; }
let calls = 0;
class Base { get v() { calls++; return 'base:' + this.tag; } get [k]() { calls++; return 'kbase:' + this.tag; } }
class Mid extends Base {}
class Leaf extends Mid { constructor() { super(); this.tag = 'leaf'; } }
const l = new Leaf();
out.push(read(l), readK(l), calls);
// a getter that throws, observed by the caller
class T { get v() { throw new Error('boom'); } get [k]() { throw new Error('kboom'); } }
class T2 extends T {}
const t = new T2();
try { read(t); } catch (e) { out.push(e.message); }
try { readK(t); } catch (e) { out.push(e.message); }
// redefine on the middle of the chain: data shadows the getter
Object.defineProperty(Mid.prototype, 'v', { value: 'mid-data', configurable: true, writable: true });
Mid.prototype[k] = 'kmid-data';
out.push(read(l), readK(l));
delete Mid.prototype.v; delete Mid.prototype[k];
out.push(read(l), readK(l));
// replace the getter itself
Object.defineProperty(Base.prototype, 'v', { get() { return 'new:' + this.tag; }, configurable: true });
Object.defineProperty(Base.prototype, k, { get() { return 'knew:' + this.tag; }, configurable: true });
out.push(read(l), readK(l));
// own data on the receiver shadows
l.v = 'own'; out.push(read(l));
const l2 = new Leaf(); Object.defineProperty(l2, k, { value: 'kown' }); out.push(readK(l2));
// getter returning arguments.length and this identity; native getters
class A { get v() { return [arguments.length, this === a2]; } }
class B extends A {}
const a2 = new B(); out.push(JSON.stringify(read(a2)));
const m = new (class extends Map {})([[1, 2]]); let s; for (let i = 0; i < 4; i++) s = m.size; out.push(s);
// a bound getter and a generator-free async-free getter of another realm-like proto
const bound = Object.create(Object.create({ get v() { return 'plain:' + (this === bound); } }));
out.push(read(bound));
// deep chain (six links)
let proto = { get v() { return 'deep'; } };
for (let i = 0; i < 6; i++) proto = Object.create(proto);
out.push(read(proto));
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "inherited-getters.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"["base:leaf","kbase:leaf",8,"boom","kboom","mid-data","kbase:leaf","base:leaf","kbase:leaf","new:leaf","knew:leaf","new:leaf","kown","[0,true]",1,"plain:true","deep"]"#]);
}

#[test]
fn call_apply_and_reflect_apply_push_callees_of_other_programs() {
    // `call`, `apply` and `Reflect.apply` of user functions compiled as other programs run
    // as in-loop frames in their program; receivers, argument lists, holes, getters on the
    // list, throws, bound targets and forwarded `arguments` keep the natives' results.
    let source = r#"
const out = [];
const other = new Function('a', 'b', '"use strict"; return [this === undefined ? "u" : typeof this, a, b, arguments.length].join(":");');
const sloppy = new Function('a', 'return [typeof this, a, arguments.length].join(":");');
const thrower = new Function('x', 'throw new RangeError("r" + x);');
const recur = new Function('n', 'return n <= 0 ? 0 : n + Reflect.apply(arguments.callee, null, [n - 1]);');
function local(a, b) { return [this && this.tag, a, b].join(":"); }
function run(i) {
  const o = { tag: 't' + i };
  out.push(other.call(o, i, 'x'), other.call(undefined, i), other.apply(o, [i, 'y']), other.apply(null),
    Reflect.apply(other, o, [i, 'z', 'extra']), sloppy.call(null, i), sloppy.apply(5, [i]),
    Reflect.apply(sloppy, 'str', []), local.call(o, i, 1), Reflect.apply(local, o, [i, 2]), recur(3));
  try { Reflect.apply(thrower, null, [i]); } catch (e) { out.push(e.name + e.message); }
  try { thrower.apply(null, [i]); } catch (e) { out.push(e.name + e.message); }
  try { Reflect.apply(other, null, undefined); } catch (e) { out.push(e.constructor.name); }
  try { Reflect.apply({}, null, []); } catch (e) { out.push(e.constructor.name); }
  out.push(Reflect.apply(other, o, { length: 2, 0: 'al', 1: 'b' }), other.apply(o, [, 'hole']));
  const getterList = [1, 2]; Object.defineProperty(getterList, 0, { get() { out.push('g'); return 'got'; } });
  out.push(Reflect.apply(other, o, getterList));
  const bound = other.bind(o, 'B'); out.push(bound(i), bound.call(null, 'c'), Reflect.apply(bound, null, ['d']));
  const args = (function () { return Reflect.apply(other, o, arguments); })('A1', 'A2'); out.push(args);
}
for (let i = 0; i < 3; i++) run(i);
// EventEmitter-like dispatch through ReflectApply from another program
const emit = new Function('handler', 'self', 'args', 'return Reflect.apply(handler, self, args);');
const seen = []; for (let i = 0; i < 3; i++) emit(function (a, b) { seen.push(this.n + a + b); }, { n: i }, ['-', i]);
out.push(seen.join());
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "cross-program-applies.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"["object:0:x:2","u:0::1","object:0:y:2","object:::0","object:0:z:3","object:0:1","object:0:1","object::0","t0:0:1","t0:0:2",6,"RangeErrorr0","RangeErrorr0","TypeError","TypeError","object:al:b:2","object::hole:2","g","object:got:2:2","object:B:0:2","object:B:c:2","object:B:d:2","object:A1:A2:2","object:1:x:2","u:1::1","object:1:y:2","object:::0","object:1:z:3","object:1:1","object:1:1","object::0","t1:1:1","t1:1:2",6,"RangeErrorr1","RangeErrorr1","TypeError","TypeError","object:al:b:2","object::hole:2","g","object:got:2:2","object:B:1:2","object:B:c:2","object:B:d:2","object:A1:A2:2","object:2:x:2","u:2::1","object:2:y:2","object:::0","object:2:z:3","object:2:1","object:2:1","object::0","t2:2:1","t2:2:2",6,"RangeErrorr2","RangeErrorr2","TypeError","TypeError","object:al:b:2","object::hole:2","g","object:got:2:2","object:B:2:2","object:B:c:2","object:B:d:2","object:A1:A2:2","0-0,1-1,2-2"]"#]);
}

#[test]
fn json_stringify_fast_path_keeps_the_full_algorithms_results() {
    // JSON.stringify of plain data takes the direct writer; replacers, gaps, toJSON on any link, accessors,
    // holes, arguments objects, boxed primitives, BigInt, cycles and depth keep the full algorithm.
    let source = r#"
const out = [];
const S = (v, ...a) => out.push(String(JSON.stringify(v, ...a)));
S({ a: 1, b: 'two', c: [1, 2, 3], d: { e: true } }); S([]); S({}); S(null); S(undefined); S(() => 1); S(Symbol('x'));
S({ u: undefined, f() {}, s: Symbol('s'), n: null, nan: NaN, inf: -Infinity, z: -0, big: 1e21, small: 1e-7 });
S([undefined, () => 1, Symbol('y'), NaN]); S('quote " back \\ nl \n tab \t ctl \u0001 uni é 😀 lone \ud800');
S({ 2: 'b', 1: 'a', x: 'x', 0: 'z' }); S({ 'ke"y': 1, 'k\u0002': 2, 'é': 3 });
const o = { a: 1 }; Object.defineProperty(o, 'hidden', { value: 2, enumerable: false }); Object.defineProperty(o, 'g', { get() { return 'got'; }, enumerable: true }); S(o);
S({ d: new Date(0) }); S({ t: { toJSON(k) { return 'tj:' + k; } } }); S([1, , 3]); const sp = [1]; sp[5] = 6; S(sp);
(function () { S(arguments); })(1, 2); S(Object.create({ inherited: 1 })); S(new Map([[1, 2]])); S(new Number(5)); S(new String('s')); S(Object(true));
try { S({ b: 10n }); } catch (e) { out.push(e.constructor.name); }
const cyc = { a: {} }; cyc.a.self = cyc; try { S(cyc); } catch (e) { out.push(e.constructor.name); }
let deep = {}; let cur = deep; for (let i = 0; i < 100; i++) { cur.n = {}; cur = cur.n; } out.push(JSON.stringify(deep).length);
S({ a: [1, { b: 2 }] }, null, 2); S({ a: 1, b: 2 }, ['b']); S({ a: 1 }, (k, v) => (k === 'a' ? v + 1 : v));
Object.prototype.toJSON = function () { return 'proto'; }; S({ a: 1 }); delete Object.prototype.toJSON;
Array.prototype.toJSON = function () { return 'arr'; }; S([1]); delete Array.prototype.toJSON;
class K { constructor() { this.x = 1; this.y = [true]; } } S(new K()); S([new K(), { z: 'w' }]);
S({ __proto__: null, a: 1 }); const p = new Proxy({ a: 1 }, {}); S(p); S(new Uint8Array([1, 2]));
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "json_stringify_fast_path_keeps_the_full_algorithms_results.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"["{\"a\":1,\"b\":\"two\",\"c\":[1,2,3],\"d\":{\"e\":true}}","[]","{}","null","undefined","undefined","undefined","{\"n\":null,\"nan\":null,\"inf\":null,\"z\":0,\"big\":1e+21,\"small\":1e-7}","[null,null,null,null]","\"quote \\\" back \\\\ nl \\n tab \\t ctl \\u0001 uni é 😀 lone \\ud800\"","{\"0\":\"z\",\"1\":\"a\",\"2\":\"b\",\"x\":\"x\"}","{\"ke\\\"y\":1,\"k\\u0002\":2,\"é\":3}","{\"a\":1,\"g\":\"got\"}","{\"d\":\"1970-01-01T00:00:00.000Z\"}","{\"t\":\"tj:t\"}","[1,null,3]","[1,null,null,null,null,6]","{\"0\":1,\"1\":2}","{}","{}","5","\"s\"","true","TypeError","TypeError",602,"{\n  \"a\": [\n    1,\n    {\n      \"b\": 2\n    }\n  ]\n}","{\"b\":2}","{\"a\":2}","\"proto\"","\"arr\"","{\"x\":1,\"y\":[true]}","[{\"x\":1,\"y\":[true]},{\"z\":\"w\"}]","{\"a\":1}","{\"a\":1}","{\"0\":1,\"1\":2}"]"#]);
}

#[test]
fn json_parse_literal_shapes_keep_create_data_property_semantics() {
    // JSON.parse objects allocated with their literal shape keep duplicate-key, index-key, __proto__,
    // wide-object, reviver and inherited-setter behavior.
    let source = r#"
const out = [];
const P = (s, r) => { try { const v = JSON.parse(s, r); out.push(JSON.stringify(v), Object.keys(v ?? {}).join(','), typeof v); } catch (e) { out.push(e.constructor.name); } };
P('{"a":1,"b":"two","c":[1,2,3],"d":{"e":true}}'); P('{"a":1,"a":2,"b":3}'); P('{"2":"b","1":"a","x":1}'); P('{"__proto__":{"p":1},"q":2}');
const pr = JSON.parse('{"__proto__":{"p":1}}'); out.push(Object.getPrototypeOf(pr) === Object.prototype, Object.keys(pr).join(), pr.p === undefined);
let wide = '{' + Array.from({ length: 40 }, (_, i) => `"k${i}":${i}`).join(',') + '}'; P(wide);
P('[{"x":1},{"x":2,"y":3},{"y":4,"x":5}]'); P('{"":1,"é":2,"\\u0041":3}'); P('{"a":{"b":{"c":{"d":[null,true,false,-0,1e300]}}}}');
P('{"a":1,"b":2}', (k, v) => (typeof v === 'number' ? v * 10 : v)); P('{bad}'); P('[1,]'); P('"\\ud800"'); P('  {"s":"x"}  ');
Object.defineProperty(Object.prototype, 'trap', { set(v) { out.push('setter-called'); }, configurable: true });
P('{"trap":1}'); delete Object.prototype.trap;
const a = JSON.parse('{"m":1,"n":2}'), b = JSON.parse('{"m":3,"n":4}'); out.push(Object.keys(a).join() === Object.keys(b).join()); a.z = 1; out.push(JSON.stringify(a), JSON.stringify(b));
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "json_parse_literal_shapes_keep_create_data_property_semantics.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"["{\"a\":1,\"b\":\"two\",\"c\":[1,2,3],\"d\":{\"e\":true}}","a,b,c,d","object","{\"a\":2,\"b\":3}","a,b","object","{\"1\":\"a\",\"2\":\"b\",\"x\":1}","1,2,x","object","{\"__proto__\":{\"p\":1},\"q\":2}","__proto__,q","object",true,"__proto__",true,"{\"k0\":0,\"k1\":1,\"k2\":2,\"k3\":3,\"k4\":4,\"k5\":5,\"k6\":6,\"k7\":7,\"k8\":8,\"k9\":9,\"k10\":10,\"k11\":11,\"k12\":12,\"k13\":13,\"k14\":14,\"k15\":15,\"k16\":16,\"k17\":17,\"k18\":18,\"k19\":19,\"k20\":20,\"k21\":21,\"k22\":22,\"k23\":23,\"k24\":24,\"k25\":25,\"k26\":26,\"k27\":27,\"k28\":28,\"k29\":29,\"k30\":30,\"k31\":31,\"k32\":32,\"k33\":33,\"k34\":34,\"k35\":35,\"k36\":36,\"k37\":37,\"k38\":38,\"k39\":39}","k0,k1,k2,k3,k4,k5,k6,k7,k8,k9,k10,k11,k12,k13,k14,k15,k16,k17,k18,k19,k20,k21,k22,k23,k24,k25,k26,k27,k28,k29,k30,k31,k32,k33,k34,k35,k36,k37,k38,k39","object","[{\"x\":1},{\"x\":2,\"y\":3},{\"y\":4,\"x\":5}]","0,1,2","object","{\"\":1,\"é\":2,\"A\":3}",",é,A","object","{\"a\":{\"b\":{\"c\":{\"d\":[null,true,false,0,1e+300]}}}}","a","object","{\"a\":10,\"b\":20}","a,b","object","SyntaxError","SyntaxError","\"\\ud800\"","0","string","{\"s\":\"x\"}","s","object","{\"trap\":1}","trap","object",true,"{\"m\":1,\"n\":2,\"z\":1}","{\"m\":3,\"n\":4}"]"#]);
}

#[test]
fn lane_closures_of_one_function_keep_their_own_lexical_this() {
    // Arrows made by one function share its derived lane target; each keeps the `this` of
    // the call that made it, and a closure-making method's arrows see that method's receiver.
    let source = r#"
      function make() { return () => this.tag; }
      const a = { tag: 'a' }, b = { tag: 'b' };
      const out = [];
      for (let i = 0; i < 4; i++) {
        const fa = make.call(a), fb = make.call(b);
        out.push(fa() + fb());
        const o = { tag: 'o' + i, m() { const x = () => this.tag; return x(); } };
        out.push(o.m());
      }
      print(out.join(','));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-closure-this.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), ["ab,o0,ab,o1,ab,o2,ab,o3"]);
}

#[test]
fn lane_activations_made_frames_by_a_native_call_read_their_own_registers() {
    // A frameless activation that calls a native becomes a VM frame, in which the lane goes
    // on: the general steps it then runs in place (computed keys, spreads, object literals)
    // read and write that frame's registers, not its caller's.
    let source = r#"
      function ensure(obj, tag) {
        for (var i = 0, keys = Object.keys(obj); i < keys.length; i++) {
          var key = keys[i];
          var fns = obj[key];
          if (typeof fns === 'function') obj[key] = { enter: fns, [tag + i]: [i, ...keys] };
        }
        return obj;
      }
      function visit(n) {
        var out = [], a = 'a', b = 1.5, c = null;
        for (var j = 0; j < n; j++) {
          var visitor = ensure({ x() {}, y: { enter: j } }, 'k');
          out.push(typeof visitor.x.enter + visitor.x.k0.join('|') + visitor.y.enter + a + b + c);
        }
        return out.join(',');
      }
      print(visit(3));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-materialized-registers.js").unwrap()).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        ["function0|x|y0a1.5null,function0|x|y1a1.5null,function0|x|y2a1.5null"]
    );
}

#[test]
fn lane_raised_errors_unwind_through_callees_of_other_programs() {
    // A lane step that throws in a callee of another program, pushed in the loop, unwinds to
    // its caller's handler, which the caller's own program locates.
    let source = r#"
      function a() {} function b() {} function c() {} function d() {} function e() {}
      const other = new Function('xs', 'let n = 0; for (let i = 0; i < xs.length; i++) { n += [...xs[i]].length; } return n;');
      function run(xs) {
        try { return other(xs); } catch (error) { return error.constructor.name; }
      }
      const out = [];
      for (let k = 0; k < 4; k++) out.push(run([[1], [2, 3], k & 1 ? [4] : 5]));
      print(out.join());
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "lane-raised-other-program.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), ["TypeError,4,TypeError,4"]);
}

#[test]
fn promoted_move_coalescing_reads_liveness_of_the_code_it_rewrites() {
    // Coalescing the copies frame-register promotion leaves runs in rounds that drop `Nop`s;
    // a round after one must not judge producers by the earlier positions, or the callback
    // copied into `filter`'s argument window is lost to an earlier call's result.
    let source = r#"
      function run(ex) {
        var f = function (m) { return { d: m }; };
        Object.defineProperty(ex, "e", { value: 1 });
        const n = f({ SIGA: 1, b: 2, SIGC: 3 });
        ex.x = Object.keys(n.d).filter(k => k.startsWith("SIG"));
        return ex.x.join();
      }
      print(run({}));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "promoted-move-rounds.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), ["SIGA,SIGC"]);
}

/// A call-only closure makes no `prototype` until a callee's legacy `caller` reaches it, and
/// then has the one its creation would have made (Node v22 oracle). Closures whose value
/// escapes, is named by `arguments` or by a direct `eval` keep theirs from creation.
#[test]
fn call_only_closures_make_their_prototype_when_caller_reaches_them() {
    let source = r#"
      var seen = [];
      function probe() {
        var f = probe.caller;
        var d = Object.getOwnPropertyDescriptor(f, 'prototype');
        var names = Object.getOwnPropertyNames(f).filter(function (n) { return n !== 'arguments' && n !== 'caller'; });
        seen.push([names.join(','), typeof d.value, d.writable, d.enumerable, d.configurable,
          f.prototype.constructor === f, Object.getOwnPropertyNames(f.prototype).join(','),
          Object.getPrototypeOf(f.prototype) === Object.prototype].join('|'));
        var o = new f(true);
        seen.push(o instanceof f);
        return 1;
      }
      function callOnly(n) {
        var loop;
        var total = 0;
        loop = function (stop) { if (stop) return 0; total += n; return probe(); };
        (loop(false)), (total += 1);
        return total;
      }
      function escapes() {
        var g;
        g = function () { return 1; };
        var k;
        k = function () { return arguments.callee === k; };
        var e;
        e = function () { return 4; };
        e();
        return [typeof g.prototype, k(), eval('typeof e.prototype')].join('|');
      }
      function returned() { var r; r = function () { return 3; }; return r; }
      print(JSON.stringify([callOnly(5), escapes(), typeof returned().prototype, seen]));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "call-only-prototype.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(
        output.borrow().as_slice(),
        [r#"[6,"object|true|object","object",["length,name,prototype|object|true|false|false|true|constructor|true",true]]"#]
    );
}

/// A hoisted `var` whose `.length` is read before its assignment runs reads `undefined`
/// (Node v22 oracle): the fused field read of a promoted local counts as a read, so the
/// local's register starts `undefined`.
#[test]
fn fused_field_reads_of_promoted_locals_see_undefined_before_assignment() {
    let source = r#"
      function beforeDecl() { try { return b.length; } catch (e) { return e.constructor.name; } var b = arguments; }
      var out = [];
      for (var r = 0; r < 3; r++) out.push(String(beforeDecl(1, 2)));
      print(out.join(','));
    "#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    let program = Engine::specialize(source, "fused-read-before-assignment.js").unwrap();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["TypeError,TypeError,TypeError"]);
}

#[test]
fn absent_index_reads_of_ordinary_objects_still_see_every_holder_on_the_chain() {
    let source = r#"
      const check = (actual, expected, label) => { if (actual !== expected) throw Error(label + ': ' + actual); };
      const plain = { length: 3 };
      for (let i = 0; i < 3; i++) check(plain[i], undefined, 'plain miss');
      check(Array.from(plain, (_, i) => i * 2).join(), '0,2,4', 'array-like from');
      check(Object.create({ 7: 'p' })[7], 'p', 'prototype property');
      check(Object.create(['a', 'b'])[1], 'b', 'array prototype');
      check(Object.create(new String('xy'))[1], 'y', 'string wrapper prototype');
      check(Object.create(new Uint8Array([4, 5]))[1], 5, 'typed array prototype');
      const getter = {}; Object.defineProperty(getter, 9, { get() { return 'g'; } });
      check(Object.create(getter)[9], 'g', 'inherited getter');
      check((function () { return Object.create(arguments)[0]; })('arg'), 'arg', 'arguments prototype');
      const proxied = Object.create(new Proxy({}, { get: (_, key) => 'proxy ' + key }));
      check(proxied[3], 'proxy 3', 'proxy prototype');
      Object.prototype[11] = 'shared';
      check({}[11], 'shared', 'Object.prototype index');
      delete Object.prototype[11];
      check({}[11], undefined, 'deleted Object.prototype index');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "absent-index.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "absent-index.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn array_fill_stores_plainly_only_where_no_prototype_claims_the_index() {
    let source = r#"
      const check = (actual, expected, label) => { if (actual !== expected) throw Error(label + ': ' + actual); };
      check(Array(5).fill(7).join(), '7,7,7,7,7', 'holes');
      check([1, 2, 3, 4].fill(0, 1, -1).join(), '1,0,0,4', 'range');
      const seen = [];
      const proto = Object.create(Array.prototype);
      Object.defineProperty(proto, 2, { set(value) { seen.push(value); }, configurable: true });
      const holed = Object.setPrototypeOf([1, , , 4], proto);
      holed.fill(9);
      check(seen.join(), '9', 'inherited setter');
      check(Object.keys(holed).join(), '0,1,3', 'setter index stays a hole');
      Object.prototype[1] = 'shared';
      Object.defineProperty(Object.prototype, 1, { value: 'readonly', writable: false, configurable: true });
      const blocked = Array(3);
      let blockedThrew = false;
      try { blocked.fill(5); } catch (e) { blockedThrew = e instanceof TypeError; }
      check(blockedThrew, true, 'read-only inherited index throws');
      check(blocked.hasOwnProperty(1), false, 'read-only inherited index stays a hole');
      check(blocked[0], 5, 'indices before it store');
      delete Object.prototype[1];
      let threw = false;
      try { Object.freeze([1, 2]).fill(0); } catch (e) { threw = e instanceof TypeError; }
      check(threw, true, 'frozen');
      threw = false;
      try { Object.seal(Array(2)).fill(0); } catch (e) { threw = e instanceof TypeError; }
      check(threw, true, 'sealed holes');
      check(Object.seal([1, 2]).fill(3).join(), '3,3', 'sealed present');
      check(Array.prototype.fill.call({ length: 2 }, 1)[1], 1, 'array-like');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "array-fill.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "array-fill.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn define_property_keeps_its_values_through_collections_its_conversions_run() {
    let source = r#"
      const churn = () => { let keep = []; for (let i = 0; i < 8000; i++) keep.push({ i, s: 'x' + i }); return keep.length; };
      const results = [];
      for (let round = 0; round < 6; round++) {
        const key = { toString() { churn(); return 'k' + round; } };
        const descriptor = {
          get enumerable() { churn(); return true; },
          get get() { churn(); const marker = { round }; return () => marker.round; },
        };
        const target = Object.defineProperty({}, key, descriptor);
        results.push(target['k' + round]);
        if (!Object.getOwnPropertyDescriptor(target, 'k' + round).enumerable) throw Error('enumerable lost');
        const reflected = {};
        if (!Reflect.defineProperty(reflected, key, { get value() { churn(); return [round]; }, writable: true })) throw Error('reflect');
        if (reflected['k' + round][0] !== round) throw Error('value lost');
      }
      if (results.join() !== Array.from({ length: 6 }, (_, i) => i).join()) throw Error('getter results ' + results);
    "#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "define-roots.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "define-roots.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn object_assign_sees_prototype_setters_added_after_a_memoized_copy() {
    let source = r#"
      const check = (actual, expected, label) => { if (actual !== expected) throw Error(label + ': ' + actual); };
      const proto = {};
      const src = { a: 1, b: 2 };
      for (let i = 0; i < 4; i++) check(Object.assign(Object.create(proto), src).a, 1, 'primed');
      let hits = 0;
      Object.defineProperty(proto, 'a', { set(value) { hits += value; }, configurable: true });
      const target = Object.assign(Object.create(proto), src);
      check(hits, 1, 'setter called');
      check(Object.prototype.hasOwnProperty.call(target, 'a'), false, 'setter kept the key off the target');
      check(target.b, 2, 'other key copied');
      delete proto.a;
      Object.defineProperty(proto, 'b', { value: 0, writable: false, configurable: true });
      let threw = false;
      try { Object.assign(Object.create(proto), src); } catch (error) { threw = error instanceof TypeError; }
      check(threw, true, 'read-only inherited key throws');
      delete proto.b;
      check(Object.assign(Object.create(proto), src).b, 2, 'plain again');
      Object.defineProperty(Object.prototype, 'a', { set(value) { hits += 10 * value; }, configurable: true });
      Object.assign({}, src);
      check(hits, 11, 'Object.prototype setter');
      delete Object.prototype.a;
    "#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "assign-memo.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "assign-memo.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
fn environment_rest_gathers_nothing_from_too_few_values() {
    // A captured rest parameter lives in the environment; with fewer values than the fixed
    // parameters, it is empty (v2-web cbaf372c1 fixed an out-of-range slice here).
    let source = r#"
      function r(a, b, ...m) { const f = () => m; return f(); }
      const seen = [];
      for (let round = 0; round < 3; round++) {
        seen.push(r(), r(1), r(1, 2), r(1, 2, 3, 4));
      }
      print(JSON.stringify(seen.slice(0, 4)) + ' ' + seen.every((rest, index) => rest.length === [0, 0, 0, 2][index % 4]));
    "#;
    let program = Engine::specialize(source, "environment-rest-too-few.js").unwrap();
    for mode in [crate::ExecutionMode::Interpreted, crate::ExecutionMode::Compiled] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.set_execution_mode(mode);
        vm.execute(&program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["[[],[],[],[3,4]] true"], "{mode:?}");
    }
}

#[test]
fn set_iteration_copies_only_while_the_intrinsics_iterate() {
    let source = r#"
      const check = (actual, expected, label) => { if (actual !== expected) throw Error(label + ': ' + actual); };
      const show = (values) => values.map(String).join(',');
      const set = new Set([1, 2, 3]);
      set.delete(2);
      check(show([...set]), '1,3', 'spread skips deleted entries');
      check(show(Array.from(set)), '1,3', 'Array.from');
      const live = new Set([1, 2, 3]);
      check(show(Array.from(live, (v) => { if (v === 1) { live.delete(2); live.add(9); } return v; })), '1,3,9', 'live map');
      class Sub extends Set { *[Symbol.iterator]() { yield 'sub'; } }
      check(show([...new Sub([1])]), 'sub', 'subclass iterator');
      const own = new Set([1]);
      own[Symbol.iterator] = function* () { yield 'own'; };
      check(show(Array.from(own)), 'own', 'own iterator');
      class Arr extends Array {}
      const made = Arr.from(new Set([5, 6]));
      check(made instanceof Arr && show(made), '5,6', 'subclass target');
      const proto = Object.getPrototypeOf(new Set()[Symbol.iterator]());
      const next = proto.next;
      proto.next = function () { const step = next.call(this); if (!step.done) step.value = 'p' + step.value; return step; };
      check(show([...new Set([1, 2])]), 'p1,p2', 'patched next spread');
      check(show(Array.from(new Set([1, 2]))), 'p1,p2', 'patched next Array.from');
      check(show(Array.from(new Set([1, 2]), (v) => v)), 'p1,p2', 'patched next mapped');
      check(show([...new Set(new Set([1]))]), 'pp1', 'patched next in Set constructor');
      proto.next = next;
      const method = Set.prototype[Symbol.iterator];
      Set.prototype[Symbol.iterator] = function* () { yield 'proto'; };
      check(show(Array.from(new Set([1]))), 'proto', 'patched @@iterator');
      Set.prototype[Symbol.iterator] = method;
      check(Math.max(...new Set([4, 7])), 7, 'spread call');
      check(show([...new Set([3, 3, -0, NaN, NaN])]), '3,0,NaN', 'Set constructor canonicalizes');
      check(Object.is([...new Set([-0])][0], 0), true, 'Set constructor stores -0 as +0');
      const add = Set.prototype.add;
      let added = 0;
      Set.prototype.add = function (value) { added++; return add.call(this, value); };
      check(new Set([1, 2]).size + added, 4, 'patched add');
      Set.prototype.add = add;
      const map = new Map([[1, 'a'], [1, 'b'], [-0, 'z'], [[2], 'c']]);
      check(map.get(1) + map.get(0) + map.size, 'bz3', 'Map constructor');
      check(new Map([[1]]).get(1), undefined, 'short entry');
      Array.prototype[1] = 'inherited';
      check(new Map([[1]]).get(1), 'inherited', 'short entry reads the prototype');
      delete Array.prototype[1];
      const entry = [5];
      Object.defineProperty(entry, 1, { get() { return 'got'; } });
      check(new Map([entry]).get(5), 'got', 'entry getter');
      let threw = false;
      try { new Map([1]); } catch (error) { threw = error instanceof TypeError; }
      check(threw, true, 'non-object entry');
      check(new Map(new Map([[7, 8]])).get(7), 8, 'Map from Map');
      check(new Map(Object.entries({ k: 1 })).get('k'), 1, 'Map from entries');
    "#;
    for program in [Engine::compile(crate::ExecutionRequest { source, name: "set-iteration.js", kind: crate::SourceKind::Script }).unwrap(), Engine::specialize(source, "set-iteration.js").unwrap()] {
        Vm::new(TestHost::default()).execute(&program).unwrap();
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn js_function_tiers_follow_the_memory_policy() {
    for policy in crate::MemoryPolicy::ALL {
        let mut vm = Vm::new(TestHost::default());
        vm.set_memory_policy(policy);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        assert_eq!(vm.js_tier_runs(), super::compiled::js_function_tiers_compile(policy), "{policy:?}");
    }
    assert!(super::compiled::js_function_tiers_compile(crate::MemoryPolicy::Throughput));
    assert!(!super::compiled::js_function_tiers_compile(crate::MemoryPolicy::MemoryFirst));
}

#[test]
fn plain_data_equality_proves_only_plain_data() {
    // Each row: [left, right, proved]. A proof needs plain data on both sides; anything else
    // stays unproved, even when a structural comparison would call it equal.
    let source = r#"
      const sym = Symbol('s');
      const holey = [1, , 3];
      const extra = [1]; extra.foo = 1;
      const hidden = { a: 1 }; Object.defineProperty(hidden, 'h', { value: 1, enumerable: false });
      const cycle = { a: 1 }; cycle.self = cycle;
      const cycle2 = { a: 1 }; cycle2.self = cycle2;
      class K { constructor() { this.a = 1; } }
      globalThis.rows = [
        [1, 1, true], [NaN, NaN, true], [0, -0, false], ['a', 'a', true], [10n, 10n, true],
        [[1, [2, 'x']], [1, [2, 'x']], true], [[1, 2], [1, 3], false], [[1], [1, 2], false],
        [{ a: 1, b: { c: [1] } }, { b: { c: [1] }, a: 1 }, true], [{ a: 1 }, { a: 2 }, false],
        [{ a: 1 }, { a: 1, b: undefined }, false], [{ a: undefined }, { b: undefined }, false],
        [JSON.parse('[{"a":"x"}]'), [{ a: 'x' }], true],
        [hidden, { a: 1 }, true],
        [holey, [1, undefined, 3], false], [extra, [1], false],
        [{ a: 1, [sym]: 1 }, { a: 1, [sym]: 1 }, false], [{ get a() { return 1; } }, { a: 1 }, false],
        [{ constructor: Object }, { constructor: Object }, false], [new K(), new K(), false],
        [Object.create(null), Object.create(null), false], [new Date(1), new Date(1), false],
        [new Error('x'), new Error('x'), false], [new Number(1), new Number(1), false],
        [cycle, cycle2, false], [[1], { 0: 1 }, false],
      ];
    "#;
    let program = Engine::specialize(source, "plain-equality.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let rows_atom = vm.intern_atom("rows");
    let rows = vm.plain_named_get(vm.realm.globals, rows_atom).unwrap();
    let rows = vm.present_dense_elements(rows, 0..vm.heap.array(rows).unwrap().elements().len()).unwrap().to_vec();
    for (index, row) in rows.into_iter().enumerate() {
        let row = vm.present_dense_elements(row, 0..3).unwrap().to_vec();
        assert_eq!(Some(vm.plain_data_equal(row[0], row[1])), row[2].as_bool(), "row {index}");
    }
}

#[test]
fn array_holes_follow_prototype_chain_mutations() {
    // Hole reads and stores consult a proof that the chain holds no index
    // (`chain_holds_no_index`); every mutation below must retire or bypass it. Expected
    // output is Node's (v22).
    let source = r#"
// Oracle: holes read through prototype chains under mutation.
const out = [];
const a = new Array(4); out.push(a[1]);
Array.prototype[1] = "proto1"; out.push(a[1]); delete Array.prototype[1]; out.push(a[1]);
Object.prototype[2] = "obj2"; out.push(a[2]); delete Object.prototype[2]; out.push(a[2]);
Object.defineProperty(Array.prototype, 3, { get() { return "getter3"; }, configurable: true }); out.push(a[3]);
a[3] = "set3"; out.push(a[3], Object.getOwnPropertyDescriptor(a, 3) === undefined);
delete Array.prototype[3]; out.push(a[3]);
Object.defineProperty(Object.prototype, 0, { set(v) { out.push("setter0:" + v); }, configurable: true });
const b = new Array(2); b[0] = "x"; out.push(b.hasOwnProperty(0)); delete Object.prototype[0];
b[0] = "y"; out.push(b[0]);
Object.defineProperty(Array.prototype, 1, { value: "ro", writable: false, configurable: true });
const c = []; c.length = 3; c[1] = "w"; out.push(c[1], c.hasOwnProperty(1)); delete Array.prototype[1];
Array.prototype.length = 0; Array.prototype.push("pushed"); out.push(new Array(3)[0]); Array.prototype.length = 0;
out.push(new Array(3)[0]);
const p = new Proxy({}, { get(t, k) { return "proxy:" + String(k); } });
const d = new Array(2); Object.setPrototypeOf(d, p); out.push(d[1]);
Object.setPrototypeOf(d, Array.prototype); out.push(d[1]);
const sp = []; sp[100000] = 1; const e = new Array(5); Object.setPrototypeOf(e, sp); out.push(e[100000], e[4]);
for (let i = 0; i < 3; i++) { const f = new Array(3); out.push(f[i]); if (i === 0) Array.prototype[1] = "late"; }
delete Array.prototype[1];
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "array-holes.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"[null,"proto1",null,"obj2",null,"getter3","getter3",true,null,"setter0:x",false,"y","ro",false,"pushed",null,"proxy:1",null,1,null,null,"late",null]"#]);
}

#[test]
fn cached_accessor_reads_follow_getter_redefinitions() {
    // A site's accessor entry keeps whether the holder's slot holds the getter
    // (`AccessorFieldCache::getter_in_slot`); redefined, shadowed and own getters must still
    // be the ones called. Expected output is Node's (v22).
    let source = r#"
const out = [];
class E { constructor(k) { this._k = k; } get key() { return this._k; } }
const items = [new E(1), new E(2), new E(3)];
const read = (o) => o.key;
for (let r = 0; r < 4; r++) {
  let sum = 0; for (const it of items) sum += read(it); out.push(sum);
  if (r === 0) Object.defineProperty(E.prototype, "key", { get() { return this._k * 10; }, configurable: true });
  if (r === 1) Object.defineProperty(items[1], "key", { get() { return 100; }, configurable: true });
  if (r === 2) Object.defineProperty(E.prototype, "key", { value: 7, configurable: true, writable: true });
}
// own accessors: each receiver of a shape keeps its own getter in the slot
const mk = (v) => { const o = {}; Object.defineProperty(o, "g", { get() { return v; }, configurable: true, enumerable: true }); return o; };
const own = [mk("a"), mk("b"), mk("c")]; const rg = (o) => o.g;
for (let r = 0; r < 3; r++) { out.push(own.map(rg).join("")); if (r === 0) Object.defineProperty(own[0], "g", { get() { return "z"; }, configurable: true }); if (r === 1) Object.defineProperty(own[2], "g", { set(v) {}, configurable: true }); }
// inherited through two levels, holder getter swapped by assignment to a setter-less accessor slot replacement
class A { get v() { return "A"; } } class B extends A {} class C extends B {}
const cs = [new C(), new C()]; const rv = (o) => o.v;
for (let r = 0; r < 3; r++) { out.push(cs.map(rv).join("")); if (r === 0) Object.defineProperty(A.prototype, "v", { get() { return "A2"; }, configurable: true }); if (r === 1) Object.defineProperty(B.prototype, "v", { get() { return "B"; }, configurable: true }); }
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "accessor-reads.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"[6,60,140,114,"abc","zbc","zbc","AA","A2A2","BB"]"#]);
}

#[test]
fn typeof_against_typeof_compares_like_the_names() {
    // `typeof a === typeof b` and its other equality forms compare the names' indices
    // (`typeof_test`); results and operand order match Node's (v22).
    let source = r#"
const vals = [1, 1.5, "s", true, null, undefined, {}, [], () => 1, Symbol("x"), 10n, NaN, function(){}, new Proxy({}, {}), new Proxy(function(){}, {})];
const out = [];
for (const a of vals) for (const b of vals) out.push(typeof a === typeof b ? 1 : 0, typeof a != typeof b ? 1 : 0, (typeof a == typeof b) + 0, (typeof a !== typeof b) + 0);
let order = []; const t = { get x() { order.push("x"); return 1; }, get y() { order.push("y"); return "s"; } };
out.push(typeof t.x === typeof t.y, order.join(""));
out.push(typeof undeclaredName === typeof undefined, typeof undeclaredName !== typeof notDeclaredEither);
if (typeof vals[0] === typeof vals[1]) out.push("if-same"); if (!(typeof vals[0] !== typeof vals[2])) out.push("bad"); else out.push("if-diff");
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "typeof-pairs.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"[1,0,1,0,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,1,0,1,0,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,1,0,1,0,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,1,0,1,0,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,1,0,1,0,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,0,1,0,1,0,1,0,1,1,0,1,0,0,1,0,1,1,0,1,0,false,"xy",true,false,"if-same","if-diff"]"#]);
}

#[test]
fn field_stores_to_cell_receivers_follow_their_descriptors() {
    // A store site whose receiver lives outside the object records (a RegExp's or a
    // function's cell) moves to the general cached write (`lane_set_field_miss`); stores
    // there must still honour read-only and frozen receivers. Expected output is Node's (v22).
    let source = r#"
const out = [];
const re = /a/y, fn = function () {}, arr = [1, 2];
const targets = [re, fn, arr, {}, new Map(), Object.create(null)];
function put(o, v) { o.lastIndex = v; o.tag = v; }
for (let r = 0; r < 40; r++) for (const t of targets) put(t, r);
for (const t of targets) out.push(t.lastIndex, t.tag);
Object.defineProperty(re, "lastIndex", { writable: false });
try { (function () { "use strict"; re.lastIndex = 5; })(); } catch (e) { out.push(e.constructor.name); }
re.lastIndex = 6; out.push(re.lastIndex);
Object.freeze(fn); put(fn, 99); out.push(fn.lastIndex, fn.tag);
const re2 = /b/g; for (let i = 0; i < 30; i++) { re2.lastIndex = i; out.push(re2.lastIndex === i); }
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "cell-stores.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"[39,39,39,39,39,39,39,39,39,39,39,39,"TypeError",39,39,39,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true,true]"#]);
}

#[test]
fn js_general_artifact_key_tracks_only_code_generation_dependencies() {
    fn function_id(program: &crate::bytecode::ResidualProgram, name: &str) -> u32 {
        program
            .functions
            .iter()
            .position(|function| {
                function
                    .header()
                    .name
                    .or(function.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .expect("test function exists") as u32
    }

    let first = Engine::compile(crate::ExecutionRequest::script(
        "const unrelated = 'first'; function f(value) { return value + 1; }",
        "general-cache-a.js",
    ))
    .unwrap();
    let equivalent = Engine::compile(crate::ExecutionRequest::script(
        "const unrelated = 'second'; function f(value) { return value + 1; }",
        "general-cache-b.js",
    ))
    .unwrap();
    let different_constant = Engine::compile(crate::ExecutionRequest::script(
        "const unrelated = 'first'; function f(value) { return value + 2; }",
        "general-cache-c.js",
    ))
    .unwrap();

    let first_key = super::compiled::js_general_dispatch_cache_key(
        &first,
        function_id(&first, "f"),
        true,
    )
    .expect("small function has a bounded cache key");
    let equivalent_key = super::compiled::js_general_dispatch_cache_key(
        &equivalent,
        function_id(&equivalent, "f"),
        true,
    )
    .expect("equivalent function has a bounded cache key");
    let different_key = super::compiled::js_general_dispatch_cache_key(
        &different_constant,
        function_id(&different_constant, "f"),
        true,
    )
    .expect("different function has a bounded cache key");

    assert_eq!(first_key, equivalent_key);
    assert_ne!(first_key, different_key);
}

#[test]
fn synthesized_throws_class_setup_and_using_use_the_realm_intrinsics() {
    // The compiler's own TypeError and ReferenceError throws, class prototype setup and
    // `using` declarations call the realm's intrinsics, not whatever binding of the same name
    // is in scope (`INTRINSIC_TYPE_ERROR_BINDING` and its siblings). Expected output is Node's
    // (v26.12).
    for (source, expected) in [
        (r#"const out = [];
{
  class TypeError { constructor(m) { this.fake = m; } }
  const c = 1;
  try { c = 2; } catch (e) { out.push(e instanceof globalThis.TypeError, e.fake === undefined, e.constructor.name); }
  try { const { a } = null; } catch (e) { out.push(e instanceof globalThis.TypeError, e.fake === undefined); }
  try { (function () { const [x] = undefined; })(); } catch (e) { out.push(e instanceof globalThis.TypeError, e.fake === undefined); }
}
print(JSON.stringify(out));
"#, r#"[true,true,"TypeError",true,true,true,true]"#),
        (r#"const out = [];
{
  const Object = { setPrototypeOf() { out.push("fake setPrototypeOf"); } };
  class B { static s() { return 1; } }
  class A extends B {}
  out.push(A.s(), globalThis.Object.getPrototypeOf(A) === B);
  class N extends null {}
  out.push(globalThis.Object.getPrototypeOf(N.prototype) === null);
}
const realSet = Object.setPrototypeOf;
Object.setPrototypeOf = function () { out.push("patched"); return realSet.apply(this, arguments); };
class C extends Array {}
out.push(new C() instanceof Array);
Object.setPrototypeOf = realSet;
{
  class DisposableStack { constructor() { out.push("fake stack"); } }
  try {
    { using r = { [Symbol.dispose]() { out.push("disposed"); } }; out.push("in"); }
  } catch (e) { out.push("E:" + e.constructor.name); }
}
print(JSON.stringify(out));
"#, r#"[1,true,true,true,"in","disposed"]"#),
        (r#"const out = [];
{
  const ReferenceError = function () { this.fake = 1; };
  try { (function () { super_delete: { } })(); } catch (e) {}
  try { eval("f() = 1"); } catch (e) { out.push(e instanceof globalThis.ReferenceError, e.fake === undefined); }
  class P { m() { try { delete super.x; } catch (e) { out.push(e instanceof globalThis.ReferenceError, e.fake === undefined); } } }
  new P().m();
}
print(JSON.stringify(out));
"#, r#"[true,true,true,true]"#),
    ] {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(output.clone()));
        vm.execute(&Engine::specialize(source, "intrinsic-names.js").unwrap()).unwrap();
        assert_eq!(output.borrow().as_slice(), [expected]);
    }
}

#[test]
fn comparisons_with_a_constant_on_the_left_compare_like_node() {
    // `C < r` compiles as `r > C` (`constant_right`), as values and as branch tests; results
    // and conversion calls match Node's (v26.12).
    let source = r#"
const vals = [0, -0, 1, -1, 2.5, NaN, Infinity, -Infinity, "1", "a", "", null, undefined, true, false, 10n, {valueOf() { return 3; }}, [2], [], Symbol.iterator];
const out = [];
for (const v of vals) {
  const r = [];
  for (const f of [x => 1 < x, x => 1 > x, x => 1 <= x, x => 1 >= x, x => 1 == x, x => 1 != x, x => 1 === x, x => 1 !== x,
                   x => "1" < x, x => null >= x, x => undefined == x, x => NaN < x, x => 0 === x, x => -0 >= x, x => 2n > x]) {
    try { r.push(f(v) ? 1 : 0); } catch (e) { r.push("E"); }
    try { if (f(v)) r.push("t"); else r.push("f"); } catch (e) { r.push("E"); }
  }
  out.push(r.join(""));
}
let calls = 0; const o = { valueOf() { calls++; return 5; } };
out.push(3 < o, 3 > o, 5 <= o, calls);
for (let i = 0; 64 > i; i++) calls++;
out.push(calls);
print(JSON.stringify(out));
"#;
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "constant-left.js").unwrap()).unwrap();
    assert_eq!(output.borrow().as_slice(), [r#"["0f1t0f1t0f1t0f1t0f1t0f0f1t1t1t","0f1t0f1t0f1t0f1t0f1t0f0f1t1t1t","0f0f1t1t1t0f1t0f0f0f0f0f0f0f1t","0f1t0f1t0f1t0f1t0f1t0f0f0f1t1t","1t0f1t0f0f1t0f1t1t0f0f0f0f0f0f","0f0f0f0f0f1t0f1t0f0f0f0f0f0f0f","1t0f1t0f0f1t0f1t1t0f0f0f0f0f0f","0f1t0f1t0f1t0f1t0f1t0f0f0f1t1t","0f0f1t1t1t0f0f1t0f0f0f0f0f0f1t","0f0f0f0f0f1t0f1t1t0f0f0f0f0f0f","0f1t0f1t0f1t0f1t0f1t0f0f0f1t1t","0f1t0f1t0f1t0f1t0f1t1t0f0f1t1t","0f0f0f0f0f1t0f1t0f0f1t0f0f0f0f","0f0f1t1t1t0f0f1t0f0f0f0f0f0f1t","0f1t0f1t0f1t0f1t0f1t0f0f0f1t1t","1t0f1t0f0f1t0f1t1t0f0f0f0f0f0f","1t0f1t0f0f1t0f1t1t0f0f0f0f0f0f","1t0f1t0f0f1t0f1t1t0f0f0f0f0f0f","0f1t0f1t0f1t0f1t0f1t0f0f0f1t1t","EEEEEEEE0f1t0f1tEEEE0fEE0fEEEE",true,false,true,3,67]"#]);
}

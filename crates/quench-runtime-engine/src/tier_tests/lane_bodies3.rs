//! Lane bodies against the interpreter: each program runs compiled and interpreted, and the compiled run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::test_oracle::{assert_tier_agrees, Tier};

#[test]
fn lane_capture_stores_write_like_the_interpreter() {
    // A lane body stores to captures of enclosing scopes, and a callee it calls directly
    // resolves its own captures in its closure's scope, not its caller's.
    let source = r#"
      function scopeB() {
        let b = 100;
        let hits = 0;
        function g(x) { b = b + x; hits = hits + 1; if (x > 3) return g(x - 3) + b; return b + hits; }
        return { g, read: () => b * 1000 + hits };
      }
      function scopeA(g) {
        let a = 1;
        let log = null;
        function f(x) { log = { x, next: log }; a = a + (x & 1); return g(x) + a; }
        return { f, size: () => { let n = 0; for (let l = log; l !== null; l = l.next) n++; return n + a; } };
      }
      const B = scopeB(), A = scopeA(B.g);
      let total = 0;
      for (let i = 0; i < 4000; i++) { total = (total + A.f(i % 13)) % 1000000007; }
      globalThis.answer = (total * 7 + B.read() * 3 + A.size());
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(1843176980.0));
}

#[test]
fn lane_inherited_constants_follow_prototype_writes() {
    // A lane body takes an inherited method or value as the constant its prototype held when
    // it compiled, only while that prototype is unchanged: a value write (callable or not), a
    // swapped prototype and a change up the chain each read what the interpreter reads.
    let source = r#"
      function Task(id) { this.id = id; this.link = null; }
      Task.prototype.run = function (x) { return x + this.id; };
      Task.prototype.weight = 3;
      function Other(id) { this.id = id; this.link = null; }
      Other.prototype.run = function (x) { return x * 2 + this.id; };
      Other.prototype.weight = 5;
      function loop(tasks, rounds) {
        let t = 0;
        for (let r = 0; r < rounds; r++) {
          for (let i = 0; i < tasks.length; i++) { const k = tasks[i]; t = (t + k.run(r) + k.weight) % 1000003; }
        }
        return t;
      }
      const tasks = []; for (let i = 0; i < 20; i++) tasks.push(i % 5 === 0 ? new Other(i) : new Task(i));
      let total = loop(tasks, 3000);
      Task.prototype.weight = 11;                       // value write, non-callable
      total += loop(tasks, 500);
      Task.prototype.run = function (x) { return x - this.id; };   // method replaced
      total += loop(tasks, 500);
      Object.setPrototypeOf(tasks[3], Other.prototype); // prototype swapped
      total += loop(tasks, 500);
      Object.prototype.weightless = 1; Task.prototype.weight = 7;  // chain change and value write
      total += loop(tasks, 500);
      globalThis.answer = total;
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(1225140.0));
}

#[test]
fn lane_array_length_reads_dense_length_only() {
    // A lane body reads a direct array's `length` from its record only while the array's
    // dense elements give it: a grown or truncated array, one that has held sparse elements,
    // an arguments object, a plain object and a string each read what the interpreter reads.
    let source = r#"
      function count(xs, rounds) {
        let t = 0;
        for (let r = 0; r < rounds; r++) {
          for (let i = 0; i < xs.length; i++) { t = (t + i + xs.length) % 1000003; }
        }
        return t;
      }
      function args() { return count(arguments, 50); }
      const dense = []; for (let i = 0; i < 20; i++) dense.push(i);
      let total = count(dense, 2000);
      dense.push(7); dense.length = 15;
      total += count(dense, 500);
      const sparse = [1, 2, 3]; sparse[60000] = 4;
      total += count(sparse, 1);
      sparse.length = 5;
      total += count(sparse, 500);
      total += args(1, 2, 3, 4);
      total += count({ length: 6 }, 500);
      total += count("hello", 500);
      globalThis.answer = total;
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(540398.0));
}

#[test]
fn lane_hoisted_length_survives_inlined_calls() {
    // A loop's `length` read made once on its entry edge (`invariant_reads`) holds through a
    // body that reads elements and runs an inlined call, as the interpreter's reads do.
    let source = r#"
      function Task(id) { this.id = id; }
      Task.prototype.run = function (x) { return (x + this.id) | 0; };
      function kernel(tasks, n) {
        let s = 0;
        for (let r = 0; r < n; r++) for (let i = 0; i < tasks.length; i++) { const k = tasks[i]; s = (s + k.run(1)) | 0; }
        return s;
      }
      const tasks = []; for (let i = 0; i < 10; i++) tasks.push(new Task(i));
      let total = 0; for (let c = 0; c < 300; c++) total = (total + kernel(tasks, 100)) | 0;
      globalThis.answer = total;
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(1650000.0));
}

#[test]
fn lane_bodies_read_static_methods_after_leave_feedback() {
    // A method reading a captured constructor's own field (a static method) leaves its first
    // body at that read on every run; the next takes the read through the VM's cache
    // (`Vm::unit_fields_in_lane`), and a replaced static method reads as the interpreter reads.
    // Inside a function, as a CommonJS module's code is, the constructor is a capture.
    let source = r#"
     (function () {
      function Strength(v) { this.v = v; }
      Strength.stronger = function (a, b) { return a.v > b.v; };
      Strength.weakestOf = function (a, b) { return this.stronger(a, b) ? b : a; };
      Strength.WEAK = new Strength(1);
      function Constraint(s) { this.strength = s; }
      Constraint.prototype.pick = function (other) { return Strength.stronger(this.strength, other) ? 1 : 2; };
      const cs = []; for (let i = 0; i < 8; i++) cs.push(new Constraint(new Strength(i)));
      let total = 0;
      for (let r = 0; r < 4000; r++) for (let i = 0; i < cs.length; i++) total += cs[i].pick(Strength.WEAK);
      Strength.stronger = function (a, b) { return a.v < b.v; };
      for (let r = 0; r < 500; r++) for (let i = 0; i < cs.length; i++) total += cs[i].pick(Strength.WEAK);
      for (let r = 0; r < 4000; r++) for (let i = 0; i < cs.length; i++) total += Strength.weakestOf(cs[i].strength, Strength.WEAK).v;
      globalThis.answer = total;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(163500.0));
}

#[test]
fn lane_loose_equality_decides_records_and_nullish_only() {
    // A lane body decides `==` and `!=` of object records, `null` and `undefined` and
    // Numbers natively; strings, Booleans, functions and objects converted to primitives
    // leave, and every pair reads what the interpreter reads.
    let source = r#"
      function P(v) { this.v = v; }
      const a = new P(1), b = new P(2), arr = [1], f = function () {};
      const values = [a, b, arr, null, undefined, 0, 1, 1.5, NaN, "1", "", true, false, f, { valueOf() { return 1; } }];
      function eq(x, y) { return x == y ? 1 : 0; }
      function ne(x, y) { if (x != y) return 2; return 3; }
      let total = 0;
      for (let r = 0; r < 300; r++)
        for (let i = 0; i < values.length; i++)
          for (let j = 0; j < values.length; j++) total += eq(values[i], values[j]) * (i + 1) + ne(values[i], values[j]) * (j + 1);
      globalThis.answer = total;
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(1290000.0));
}

#[test]
fn lane_bodies_construct_by_activation() {
    // A lane body's `new` whose constructor does more than store fields (an array literal,
    // a nested construction and call, a returned object) runs the constructor's activation
    // (`dispatch_fast::body_construct_helper`); a planned one and a returned one by the plan.
    let source = r#"
     (function () {
      function OC() { this.elms = []; }
      OC.prototype.add = function (e) { this.elms.push(e); };
      OC.prototype.size = function () { return this.elms.length; };
      function V(n) { this.n = n; this.cs = new OC(); this.cs.add(n); }
      function R(x) { this.x = x; if (x > 5) return { x: -x }; }
      function P(a, b) { this.a = a; this.b = b; }
      function make(i) { return new P(i, i + 1); }
      function f(i) {
        var a = new OC(); a.add(i); a.add(i + 1);
        var v = new V(i);
        var r = new R(i % 9);
        var p = make(i);
        return a.size() + v.cs.size() + v.n + r.x + p.b;
      }
      let t = 0; for (let i = 0; i < 40000; i++) t = (t + f(i)) | 0;
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(1600093342.0));
}

#[test]
fn lane_bodies_allocate_literals() {
    // A lane body allocates `{}` and `[]` (`compiled::compiled_new_object`, `new_array_in_lane`)
    // across many collections, and the objects it fills read as the interpreter's.
    let source = r#"
     (function () {
      function OC() { this.elms = []; }
      OC.prototype.add = function (e) { this.elms.push(e); };
      function pair(a) { var o = {}; o.a = a; o.list = []; o.list.push(a); return o; }
      function f(i) { var c = new OC(); c.add(i); c.add(pair(i)); return c.elms.length + c.elms[1].a + c.elms[1].list.length; }
      let t = 0; for (let i = 0; i < 200000; i++) t = (t + f(i)) | 0;
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(-1474336480.0));
}

#[test]
fn lane_bodies_construct_empty_arrays() {
    // `new Array()` of the realm's `Array` in a lane body is an empty array
    // (`dispatch_fast::body_construct_helper`); a function named `Array` constructs as itself,
    // and a constructor only lane bodies construct gets a body of its own.
    let source = r#"
     (function () {
      function OC() { this.elms = new Array(); }
      OC.prototype.add = function (e) { this.elms.push(e); };
      OC.prototype.size = function () { return this.elms.length; };
      function shadowed() { function Array() { this.mine = 7; } return new Array().mine; }
      function f(i) { var a = new OC(); a.add(i); a.add(i); var b = new OC(); b.add(a.size()); return a.size() + b.elms[0] + shadowed() + (a.elms instanceof Array ? 1 : 0); }
      let t = 0; for (let i = 0; i < 60000; i++) t = (t + f(i)) | 0;
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(720000.0));
}

#[test]
fn lane_bodies_add_properties_at_feedback_stores() {
    // A store a previous body left at adds its property (`compiled::field_add_in_lane`): to
    // `this`, whose shape the body reads again, and to a fresh object, and the fields read
    // back past the shape change as the interpreter's.
    let source = r#"
     (function () {
      function V(n) { this.n = n; this.list = new Array(); this.mark = 0; }
      V.prototype.tag = function (i) {
        if (i % 3 === 0) this.extra = i;
        var o = {}; o.a = this.n; o.b = i; o.c = this.mark;
        this.mark = this.mark + 1;
        return o.a + o.b + o.c + (this.extra === undefined ? 0 : this.extra) + this.list.length;
      };
      let t = 0;
      for (let r = 0; r < 3000; r++) { const v = new V(r); for (let i = 0; i < 8; i++) t = (t + v.tag(i)) | 0; }
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(36219000.0));
}

#[test]
fn lane_bodies_run_the_staged_add_row_until_a_prototype_gains_a_setter() {
    // An adding store of a constructor runs `staged::add`'s row in the lane body, holding the
    // prototype chain's proof as a Stable fact; a setter installed late retires it, and the
    // stores then reach the setter as the interpreter's do.
    let source = r#"
     (function () {
      var hits = 0;
      function W(n) { this.n = n; this.m = n + 1; this.k = n * 2; }
      let t = 0;
      for (let r = 0; r < 6000; r++) {
        if (r === 4000) Object.defineProperty(W.prototype, 'm', { set(v) { hits += v; }, get() { return 5; }, configurable: true });
        const w = new W(r & 15);
        t = (t + w.n + w.m + w.k + hits) | 0;
      }
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert!(run.outcome.answer.is_some());
}

#[test]
fn lane_bodies_read_function_fields_natively() {
    // A lane body reads a function's own fields (static methods and data) through the
    // function's record (`ObjectHeader::prove_readable`); a replaced static method, a written
    // static field and a holder that names an object instead read as the interpreter reads.
    let source = r#"
     (function () {
      function Strength(v) { this.v = v; }
      Strength.stronger = function (a, b) { return a.v < b.v; };
      Strength.weakestOf = function (a, b) { return this.stronger(a, b) ? b : a; };
      Strength.LIMIT = 3;
      var holder = { S: Strength };
      function pick(a, b) { return Strength.weakestOf(a, b).v + Strength.LIMIT + holder.S.LIMIT; }
      var a = new Strength(1), b = new Strength(5);
      let t = 0;
      for (let i = 0; i < 20000; i++) t += pick(a, b);
      Strength.LIMIT = 10;
      for (let i = 0; i < 3000; i++) t += pick(a, b);
      Strength.stronger = function (x, y) { return x.v > y.v; };
      for (let i = 0; i < 3000; i++) t += pick(a, b);
      holder.S = { LIMIT: 100 };
      for (let i = 0; i < 3000; i++) t += pick(a, b);
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(691000.0));
}

#[test]
fn lane_inherited_reads_fold_deep_sibling_chains() {
    // Receivers of sibling classes share one field shape and differ in prototype, whose
    // methods sit one to three links up; their reads fold to each prototype's value
    // (`Vm::native_field_read_descriptors`), and a replaced override reads as the interpreter's.
    let source = r#"
     (function () {
      function inherits(child, parent) { function I() {} I.prototype = parent.prototype; child.prototype = new I(); }
      function C(s) { this.s = s; }
      C.prototype.mark = function () { return this.s + 1; };
      C.prototype.run = function () { return this.mark() * 2 + this.kind(); };
      function U(s) { this.s = s; } inherits(U, C);
      U.prototype.kind = function () { return 1; };
      function E(s) { this.s = s; } inherits(E, U);
      function B(s) { this.s = s; } inherits(B, C);
      B.prototype.kind = function () { return 3; };
      function S(s) { this.s = s; } inherits(S, B);
      S.prototype.mark = function () { return this.s + 100; };
      const xs = [new U(1), new E(2), new B(3), new S(4)];
      let t = 0;
      for (let r = 0; r < 20000; r++) for (let i = 0; i < xs.length; i++) t += xs[i].run();
      S.prototype.mark = function () { return this.s + 1000; };
      for (let r = 0; r < 2000; r++) for (let i = 0; i < xs.length; i++) t += xs[i].run();
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(8748000.0));
}

#[test]
fn lane_feedback_stores_take_polymorphic_own_slots() {
    // A store to `this` across receivers of two shapes, which a body left at, stores through
    // the site's class-keyed own entries (`Vm::cached_field_store_hit`), a widening
    // double among them, and every read sees what the interpreter's does.
    let source = r#"
     (function () {
      function inherits(child, parent) { function I() {} I.prototype = parent.prototype; child.prototype = new I(); }
      function B(a) { this.a = a; this.direction = 0; }
      B.prototype.choose = function (k) { this.direction = (k & 1) ? 1 : -1; if (k % 7 === 0) this.direction = 0.5; return this.direction; };
      function S(a) { this.a = a; this.direction = 0; this.scale = 2; this.offset = 3; } inherits(S, B);
      const xs = [new B(1), new S(2), new B(3), new S(4)];
      let t = 0;
      for (let r = 0; r < 30000; r++) for (let i = 0; i < xs.length; i++) t += xs[i].choose(r + i) + xs[i].direction;
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(17138.0));
}

#[test]
fn lane_equality_and_relations_of_int32_fields_and_parameters() {
    // Equalities and relations of an int32 field read and a parameter: two distinct int32
    // values are unequal without a Number decode (`Lowering::equality`), and doubles, NaN, -0
    // and large values decide every operator as the interpreter does.
    let source = r#"
     (function () {
      function V(m) { this.mark = m; }
      function score(v, mark) {
        let s = 0;
        if (v.mark == mark) s += 1; if (v.mark != mark) s += 2; if (v.mark < mark) s += 4;
        if (v.mark <= mark) s += 8; if (v.mark > mark) s += 16; if (v.mark >= mark) s += 32;
        if (mark === v.mark) s += 64; if (mark < v.mark) s += 128;
        return s;
      }
      const vs = [new V(0), new V(3), new V(-7), new V(100)];
      const marks = [0, 3, -7, 2.5, NaN, -0, 1e10, 100];
      let t = 0;
      for (let r = 0; r < 4000; r++) for (const v of vs) for (const m of marks) t += score(v, m);
      globalThis.answer = t;
     })();
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(9980000.0));
}

#[test]
fn loop_stores_to_packed_objects_write_like_the_interpreter() {
    // A loop region's stores through a field's slot, a packed object's inline word or an
    // arena slot (the loop's cached field-slot stores): Int32, double and object values written
    // there read back as the interpreter's.
    let source = r#"
      function Counter() { this.count = 0; this.step = 1; }
      function Pair(a, b) { this.car = a; this.cdr = b; }
      function bump(c, n) { for (var i = 0; i < n; i++) c.count = (c.count + c.step) | 0; return c.count; }
      function mix(p, n) { for (var i = 0; i < n; i++) { p.car = p.car + 0.5; p.cdr = (p.cdr + 1) | 0; } return p.car * 3 + p.cdr; }
      let total = 0;
      for (let r = 0; r < 400; r++) {
        const c = new Counter(); total = (total + bump(c, 100) + c.step) | 0;
        const p = new Pair(r, r * 2); total += mix(p, 50); p.car = { k: r }; total += p.car.k + p.cdr;
      }
      globalThis.answer = total;
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(748800.0));
}

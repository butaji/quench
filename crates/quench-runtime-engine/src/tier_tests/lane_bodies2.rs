//! Lane bodies against the interpreter: each program runs compiled and interpreted, and the compiled run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use super::lane_bodies6::LANE_METHOD_SITES_SOURCE;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use crate::test_oracle::{assert_tier_agrees, Tier};

#[test]
fn lane_bodies_inline_storing_callees_and_leave_inside_them_exactly() {
    // `step` calls `add`, which stores `this` fields: the call inlines into `step`'s lane
    // body. A string total makes `add` leave inside it, a double makes its int path
    // leave, and swapping `add` makes the call's guard leave at the call itself. The callee
    // captures its scale in an environment distinct from the caller's mutable bias.
    let source = r#"(function () {
      function Counter(n) { this.n = n; this.total = 0; this.calls = 0; }
      let bias = 7, coercions = 0;
      function makeAdder(scale) {
        globalThis.changeScale = function (value) { scale = value; };
        return function (k) {
        this.calls = this.calls + 1;
        this.total = this.total + k * scale;
        if (this.total > 1000) { this.total = this.total - 1000; }
        return this.total;
      }; }
      Counter.prototype.add = makeAdder(1);
      Counter.prototype.step = function (k) {
        const t = this.add(k);
        return t + this.n + bias;
      };
      const COUNTERS = 6, ROUNDS = 4000, REPLACE_AT = 3000;
      const counters = [];
      for (let i = 0; i < COUNTERS; i++) counters.push(new Counter(i));
      let sum = 0;
      for (let round = 0; round < ROUNDS; round++) {
        for (const c of counters) sum = (sum + c.step(round & 31)) % 1000003;
        if (round === 1500) counters[2].total = "x";
        if (round === 1501) counters[2].total = 0;
        if (round === 1700) changeScale({ valueOf() { coercions++; return 1; } });
        if (round === 1701) changeScale(1);
        if (round === 2200) counters[3].total = 0.25;
        if (round === 2201) counters[3].total = 0;
        if (round === REPLACE_AT) { Counter.prototype.add = makeAdder(2); bias = 11; }
      }
      let calls = 0;
      for (const c of counters) calls += c.calls;
      if (calls !== COUNTERS * ROUNDS) throw Error('repeated or lost inlined store');
      if (coercions !== COUNTERS) throw Error('repeated or lost capture coercion');
      globalThis.answer = sum + calls;
      globalThis.totals = counters.map(c => c.total).join(',');
      globalThis.inlineCounter = counters[0];
      globalThis.inlineStep = Counter.prototype.step;
    })();"#;
    let program = Engine::specialize(source, "lane-inline.js").unwrap();
    let mut answers = Vec::new();
    for mode in [quench_runtime::ExecutionMode::Interpreted, quench_runtime::ExecutionMode::Compiled] {
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        vm.programs.enable_js_lane_methods();
        vm.set_execution_mode(mode);
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let global = vm.realm.globals;
        let atom = vm.intern_atom("answer");
        let answer = vm.own_property(global, atom).map(Value::to_bits);
        let atom = vm.intern_atom("totals");
        let totals = vm.own_property(global, atom).and_then(|value| vm.heap.string(value))
            .map(ToString::to_string);
        answers.push((answer, totals));
        let caller = vm.intern_atom("inlineStep");
        let caller = vm.own_property(global, caller).unwrap();
        let callee = vm.intern_atom("inlineCounter");
        let receiver = vm.own_property(global, callee).unwrap();
        let callee = vm.intern_atom("add");
        let callee = vm.get_property(&program, receiver, callee).unwrap();
        let quench_runtime::heap::FunctionKind::User(_, caller_id) = vm.heap.function(caller).unwrap().kind else { panic!("script caller") };
        let quench_runtime::heap::FunctionKind::User(_, callee_id) = vm.heap.function(callee).unwrap().kind else { panic!("script callee") };
        let body = program.function(caller_id as usize);
        let mut targets = vec![None; body.code.len()];
        let call = body.instructions().position(|instruction| instruction.op() == quench_runtime::bytecode::Op::Call).unwrap();
        targets[call] = Some(quench_runtime::vm::engine_types::CallTarget { callee: quench_runtime::vm::engine_types::CalleeBinding::Identity(callee), function: callee_id });
        let callee_captures = vm.callee_capture_slots(callee);
        let method = vm.compile_lane_method(&program, caller_id, &|_| callee_captures, Vec::new())
            .expect("closure capture unit compiles");
        assert_eq!(method.inlined.len(), 1, "callee captures must not retain a guest call");
        let total = vm.intern_atom("total");
        let calls = vm.intern_atom("calls");
        let before_total = vm.own_property(receiver, total).unwrap().as_number().unwrap();
        let before_calls = vm.own_property(receiver, calls).unwrap().as_number().unwrap();
        let expected_total = before_total + 3.0 * 2.0;
        let expected_total = if expected_total > 1000.0 { expected_total - 1000.0 } else { expected_total };
        let env = vm.heap.function(caller).unwrap().env;
        let (slots, _) = vm.environment_capture_slots(env, false, 0).unwrap();
        let slots = slots.dense_storage().unwrap();
        let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
        let parameter = body.parameter_registers.or_else(|| body.promoted_register(0)).unwrap();
        registers[usize::from(parameter)] = Value::number(3.0);
        let instructions: Vec<_> = body.instructions().collect();
        let exit = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
            &program, registers.as_mut_ptr(), receiver, slots, std::ptr::null_mut());
        let pc = exit.settled(&mut registers, &instructions)
            .unwrap_or_else(|| panic!("inlined closure reaches caller return, not {exit:?}"));
        let terminal = body.instructions().nth(pc).unwrap();
        assert_eq!(terminal.op(), quench_runtime::bytecode::Op::Return);
        assert_eq!(registers[usize::from(terminal.register_a())], Value::number(expected_total + 11.0));
        assert_eq!(vm.own_property(receiver, total), Some(Value::number(expected_total)));
        assert_eq!(vm.own_property(receiver, calls), Some(Value::number(before_calls + 1.0)));
    }
    assert!(answers[0].0.is_some() && answers[0].1.is_some());
    assert_eq!(answers[0], answers[1]);
}

/// Lane bodies (`compiled/lane_region.rs`) run declined call-free methods on their lane
/// activations' registers and leave at exact PCs: at unsupported records, at failed guards
/// and at records that read the carried accumulator. Every result must equal the interpreter's.
#[test]
fn lane_bodies_match_the_interpreter_across_exits_and_accumulator_resumes() {
    let source = r#"(function () {
      const HELD = 4, SUSPENDED = 2, LIMIT = 3, SCALE = 7;
      function Task(id) { this.id = id; this.state = 0; this.count = 0; this.label = "t" + id; this.next = null; this.sum = 0; }
      Task.prototype.release = function () { this.state = this.state & ~HELD; return this.state; };
      Task.prototype.step = function (n) {
        this.count++;
        this.sum = this.sum + n * SCALE;
        if ((this.state & HELD) != 0 || this.state == SUSPENDED) { this.sum = this.sum - 1; }
        if (this.count < LIMIT) { this.state = this.state | HELD; return this.sum + this.count; }
        this.count = 0;
        this.state = this.state & ~HELD;
        this.sum = (this.sum + this.state + this.id) & 65535;
        return this.sum;
      };
      Task.prototype.walk = function (k) {
        let n = k;
        for (let t = this; t !== null; t = t.next) { n += t.id; n = n & 1023; }
        this.sum = this.sum + n;
        this.count = this.count + 0;
        return n + this.state + this.id + this.count;
      };
      Task.prototype.spin = function (k) {
        let a = k, b = 0;
        for (let i = 0; i < 5; i++) { b = (b + i) | 0; a++; }
        return a + b + this.id;
      };
      let head = null;
      for (let i = 0; i < 8; i++) { const t = new Task(i); t.next = head; head = t; }
      let total = 0;
      for (let round = 0; round < 3000; round++) {
        for (let t = head; t !== null; t = t.next) {
          total = (total + t.step(round & 15)) & 1048575;
          if (round % 50 === 0) total = (total + t.walk(round)) & 1048575;
          total = (total + t.spin(round % 7 === 0 ? 0.5 : round & 7)) % 1048575;
          if (round === 1500) t.state = "odd";
          if (round === 1501) t.state = 0;
          if (round === 2000) t.sum = 0.5;
        }
      }
      globalThis.answer = total;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert!(run.outcome.answer.is_some());
}

/// Lane bodies call the callees they do not inline natively (`dispatch_fast::body_call_helper`):
/// a callee that completes in its own body returns its value to the caller's native code; one
/// that leaves continues in the lane in its own activation; a callee with no body, a host
/// function or a throwing one leaves the caller at the call. Every result must equal the
/// interpreter's.
#[test]
fn lane_bodies_call_natively_and_match_the_interpreter_across_callee_exits() {
    let source = r#"(function () {
      const MASK = 65535, DEPTH = 3;
      function Node(id) { this.id = id; this.v = 0; this.next = null; this.tag = "n"; }
      Node.prototype.inner = function (k) { let a = 0; for (let i = 0; i < 3; i++) { a = (a + i + k) | 0; } this.v = (this.v + a) & MASK; return a; };
      Node.prototype.outer = function (k) {
        let r = this.inner(k) + this.inner(k + 1);
        if (k === 13) { this.tag = this.tag + k; }
        return (r + this.v) & MASK;
      };
      Node.prototype.deep = function (d) { if (d === 0) return this.id; return (this.deep(d - 1) + d) & MASK; };
      Node.prototype.risky = function (k) { if (k === 99) throw new Error("x" + k); return k + this.id; };
      let shared = 0;
      Node.prototype.bump = function (k) { for (let i = 0; i < 2; i++) { shared = (shared + k + i) & MASK; } return shared; };
      Node.prototype.reads = function (k) { const before = shared; const r = this.bump(k); return (before + shared + r) & MASK; };
      Node.prototype.tail = function (k) { const j = (k + this.id) & 7; if (j === 3) { return this.deep(1); } return this.outer(j); };
      Node.prototype.chain = function () { return this.next === null ? this.id : this.next.v & 15; };
      Node.prototype.grow = function (k) { if (k === 40 && this.extra === undefined) { this.extra = k; } return 1; };
      Node.prototype.copy = function (k) {
        this.v = (this.v + 1) & MASK;
        this.grow(k);
        this.v = (this.v + 2) & MASK;
        this.mirror = this.next === null ? 0 : this.next.v;
        if (this.next !== null) { this.peer = this.next.next; }
        return this.v + (this.mirror & 3);
      };
      Node.prototype.top = function (k) {
        let s = this.outer(k);
        s = (s + this.deep(DEPTH)) & MASK;
        s = (s + Math.max(k, this.id)) & MASK;
        s = (s + this.reads(k)) & MASK;
        s = (s + this.tail(k) + this.chain()) & MASK;
        s = (s + this.copy(k)) & MASK;
        try { s = (s + this.risky(k)) & MASK; } catch (e) { s = s + 1; }
        this.v = (this.v + s) & MASK;
        return s;
      };
      let head = null;
      for (let i = 0; i < 6; i++) { const n = new Node(i); n.next = head; head = n; }
      let total = 0;
      for (let round = 0; round < 2000; round++) {
        for (let n = head; n !== null; n = n.next) {
          total = (total + n.top(round % 101)) & 1048575;
          if (round === 1200) n.v = 0.25;
        }
      }
      globalThis.answer = total + head.tag.length;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert!(run.outcome.answer.is_some());
}

/// Lane bodies call through every form of a call record (`dispatch_fast::BodyCall`): a method site
/// (`this.add(z, x)`, whose receiver is `this`, and `cell.add(...)`'s register) whose argument
/// registers are the site's own list, not a window, in an order no window has, and a call whose
/// arguments are consecutive. Every result must equal the interpreter's, and no run may leave its
/// body at a method call.
#[test]
fn lane_bodies_call_method_sites_with_listed_arguments_like_the_interpreter() {
    let program = Engine::specialize(LANE_METHOD_SITES_SOURCE, "lane-body-method-sites.js").unwrap();
    #[cfg(not(quench_profiled))]
    let drive = program.functions.iter().position(|body|
        body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "drive")).unwrap();
    let mut answers = Vec::new();
    for mode in [quench_runtime::ExecutionMode::Interpreted, quench_runtime::ExecutionMode::Compiled] {
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        vm.programs.enable_js_lane_methods();
        vm.set_execution_mode(mode);
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let global = vm.realm.globals;
        let atom = vm.intern_atom("answer");
        answers.push(vm.own_property(global, atom).and_then(Value::as_number));
        #[cfg(not(quench_profiled))]
        if mode == quench_runtime::ExecutionMode::Compiled && quench_runtime::vm::tier_policy::tier_may_run(vm.memory_policy(), quench_runtime::jit_mode::Tier::ANY) {
            let view = vm.programs.views_in_lane(quench_runtime::vm::program_store::ProgramId::MAIN).unwrap().get(drive)
                .and_then(quench_runtime::vm::program_store::derived_lane_view).unwrap();
            // SAFETY: as above.
            assert!(matches!(unsafe { &*view.tier().tier_lane.get() }, quench_runtime::vm::engine_types::LaneBody::Specialized { .. }), "a method making method calls is published as a lane body");
        }
    }
    assert!(answers[0].is_some());
    assert_eq!(answers[0], answers[1]);
}

#[test]
fn lane_bitwise_operands_take_to_int32_of_doubles_like_the_interpreter() {
    // A double's ToInt32 is its truncation modulo 2^32 natively below 2^63 in magnitude;
    // larger magnitudes, infinities and NaN leave, and every one matches the interpreter.
    let source = r#"(function () {
      function toInt(xs, scale) { var s = 0; for (var i = 0; i < xs.length; i++) s = (s + ((xs[i] * scale) | 0)) | 0; return s; }
      function bits(x) { return ((x | 0) + (x ^ 0) + (x & -1) + (x << 0)) * 4294967296 + (x >>> 0); }
      var xs = [1.5, -1.5, -0.5, 2147483647.9, -2147483648.9, 4294967301.25, 9007199254740993,
        -9007199254740993, 1e19, -1e19, 1e300, NaN, Infinity, -Infinity, -0];
      var total = 0;
      for (var k = 0; k < 3000; k++) total = toInt(xs, 1) + bits(xs[k % xs.length] * 1);
      for (var j = 0; j < xs.length; j++) total += bits(xs[j] * 1);
      globalThis.answer = total;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert!(run.outcome.answer.is_some());
}

#[test]
fn lane_blocks_keep_the_object_proofs_all_their_edges_share_like_the_interpreter() {
    // A block that only earlier blocks enter keeps the proofs every edge into it carries:
    // an object proved before a branch is not proved again after it, while one redefined
    // or changed on any path is.
    let source = r#"(function () {
      function T(s) { this.state = s; this.a = 1; this.b = 2; this.c = 3; }
      function U() { this.pad = 0; this.state = 1; this.a = 4; this.b = 5; this.c = 6; }
      function step(t, u, k) {
        var x = t.a;
        if (t.state == 0) { t.state = 1; x = x + t.b; } else if (k & 1) { t = u; x = x + t.c; } else { t.state = 0; }
        return x + t.a + t.c + t.state;
      }
      var t = new T(0), u = new U(), total = 0;
      for (var k = 0; k < 4000; k++) total = (total + step(k % 5 ? t : u, k % 3 ? u : t, k)) | 0;
      globalThis.answer = total;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(40930.0));
}

#[test]
fn lane_global_reads_and_coercions_like_the_interpreter() {
    // A lane body reads a global as the lane does, so a reassigned, accessor or deleted
    // global reads as the interpreter's; `j++` takes ToNumeric of its operand, a double or a
    // string, and an element read of `null` takes the interpreter's TypeError.
    let source = r#"
      globalThis.SCALE = 3; var total = 0, caught = 0;
      function step(x, n) { var s = 0; for (var i = 0; i < n; i++) { var j = x; j++; s = (s + j * SCALE) | 0; } return s; }
      function index(a, k) { return a[k]; }
      for (var k = 0; k < 3000; k++) total = (total + step(k & 7, 4)) | 0;
      SCALE = 0.5;
      for (var k = 0; k < 300; k++) total = (total + step(k + 0.25, 3)) | 0;
      Object.defineProperty(globalThis, "SCALE", { get() { return 7; }, configurable: true });
      for (var k = 0; k < 300; k++) total = (total + step("5", 2)) | 0;
      for (var k = 0; k < 3000; k++) total = (total + index([k, 2], k & 1)) | 0;
      for (var k = 0; k < 20; k++) { try { total = (total + index(null, 0)) | 0; } catch (e) { caught++; } }
      delete globalThis.SCALE;
      try { step(1, 1); } catch (e) { caught += 100; }
      globalThis.answer = total + caught;
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(2506320.0));
}

#[test]
fn lane_typeof_tests_decide_like_the_interpreter() {
    // A lane body decides a fused `typeof` test natively: an immediate value by its tag, a
    // heap value by its cell, negated or not, inlined into its caller or not.
    let source = r#"
      function isNumber(n) { return typeof n === "number"; }
      function isString(s) { return typeof s === "string"; }
      function notUndefined(v) { return typeof v !== "undefined"; }
      function kinds(v) { let k = 0; if (typeof v === "object") k += 1; if (typeof v === "function") k += 2; if (typeof v === "boolean") k += 4; if (typeof v === "symbol") k += 8; if (typeof v === "bigint") k += 16; return k; }
      function score(xs) { let t = 0; for (let i = 0; i < xs.length; i++) { const v = xs[i]; if (isNumber(v)) t += 1; if (isString(v)) t += 3; if (notUndefined(v)) t += 5; t += kinds(v) * 7; } return t; }
      const values = [0, -0, 1.5, NaN, 2 ** 31, "s", "", undefined, null, true, false, {}, [], function () {}, class {}, Symbol("x"), 10n, new Number(3), new String("t"), Object.create(null)];
      let total = 0;
      for (let r = 0; r < 2000; r++) total += score(values) + r % 7;
      globalThis.answer = (total);
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(805995.0));
}

#[test]
fn lane_constructions_build_like_the_interpreter() {
    // A lane body constructs by the callee's plan: nested and returned constructions, a
    // defaulted field, more fields than an object holds inline and a constructor whose
    // result replaces the receiver build what the interpreter builds.
    let source = r#"
      function sc_Pair(car, cdr) { this.car = car; this.cdr = cdr; }
      function Point(x, y, z) { this.x = x; this.y = y; this.z = z === undefined ? 0 : z; }
      function Wide(a, b, c, d, e, f, g, h, i) { this.a = a; this.b = b; this.c = c; this.d = d; this.e = e; this.f = f; this.g = g; this.h = h; this.i = i; }
      function Odd(v) { if (v > 5) return { boxed: v }; this.v = v; }
      function build(n) { let list = null; for (let i = 0; i < n; i++) list = new sc_Pair(new sc_Pair(i, i * 2), list); return list; }
      function sum(list) { let t = 0; while (list !== null) { t += list.car.car + list.car.cdr; list = list.cdr; } return t; }
      function points(n) { let t = 0; for (let i = 0; i < n; i++) { const p = new Point(i, i + 1); const q = new Point(i, 2, 3); const w = new Wide(i, 1, 2, 3, 4, 5, 6, 7, 8); t += p.x + p.y + p.z + q.z + w.i + w.a; } return t; }
      function odd(n) { let t = 0; for (let i = 0; i < n; i++) { const o = new Odd(i % 10); t += o.v === undefined ? o.boxed : o.v; } return t; }
      function make(i) { return new sc_Pair(i, null); }
      let total = 0;
      for (let r = 0; r < 40; r++) { total += sum(build(500)) + points(200) + odd(100) + make(r).car; }
      globalThis.answer = (total);
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(17472780.0));
}

#[test]
fn lane_parser_node_constructions_build_like_the_interpreter() {
    // acorn's node construction: constructors whose fields depend on the parser's options,
    // nested constructions of a location and its positions, prototype methods and a helper
    // run through `Function.prototype.call`. Each of these handlers dispatches its successor
    // as a sibling jump (`become`), so a long run of them stays in one lane frame.
    let source = r#"
      function Position(line, col) { this.line = line; this.column = col; }
      Position.prototype.offset = function (n) { return new Position(this.line, this.column + n); };
      function SourceLocation(p, start, end) { this.start = start; this.end = end; if (p.sourceFile !== null) this.source = p.sourceFile; }
      function Node(parser, pos, loc) {
        this.type = ""; this.start = pos; this.end = 0;
        if (parser.options.locations) this.loc = new SourceLocation(parser, loc);
        if (parser.options.ranges) this.range = [pos, 0];
      }
      function finishNodeAt(node, type, pos, loc) { node.type = type; node.end = pos; if (this.options.locations) node.loc.end = loc; if (this.options.ranges) node.range[1] = pos; return node; }
      function Parser(options) { this.options = options; this.sourceFile = options.sourceFile ? "f" : null; this.pos = 0; this.line = 1; }
      Parser.prototype.curPosition = function () { if (this.options.locations) return new Position(this.line, this.pos); };
      Parser.prototype.startNode = function () { return new Node(this, this.pos, this.curPosition()); };
      Parser.prototype.finishNode = function (node, type) { return finishNodeAt.call(this, node, type, this.pos, this.curPosition()); };
      function run(options, n) {
        const p = new Parser(options);
        let t = 0;
        for (let i = 0; i < n; i++) {
          const node = p.startNode();
          p.pos += 3; if (i % 7 === 0) p.line++;
          p.finishNode(node, i & 1 ? "Identifier" : "Literal");
          t += node.end - node.start + node.type.length;
          if (node.loc) t += node.loc.start.column + node.loc.end.line + (node.loc.source ? 1 : 0) + node.loc.start.offset(2).column;
          if (node.range) t += node.range[1];
        }
        return t;
      }
      let total = 0;
      for (let r = 0; r < 8; r++) {
        total += run({ locations: true, ranges: true, sourceFile: r & 1 }, 200);
        total += run({ locations: false, ranges: r % 3 === 0 }, 150);
      }
      globalThis.answer = total;
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(1600989.0));
}

#[test]
fn lane_nested_lengths_read_like_the_interpreter() {
    // A tokenizer's `this.input.length`: a nested read whose last step is a string's own
    // `length`, of empty, ASCII and wide strings, and an array's at the same sites, which
    // read what the interpreter reads.
    let source = r#"
      function Tokenizer(input) { this.input = input; this.pos = 0; }
      Tokenizer.prototype.remaining = function () { return this.input.length - this.pos; };
      function scan(t) { let n = 0; while (t.pos < t.input.length) { t.pos++; n++; } return n; }
      const inputs = ["", "abc", "x".repeat(70), "é€😀", [1, 2, 3, 4], [], new Array(9)];
      let total = 0;
      for (let r = 0; r < 300; r++) {
        for (const input of inputs) {
          const t = new Tokenizer(input);
          total += t.remaining() * 3 + scan(t) + t.remaining() + t.input.length;
        }
      }
      globalThis.answer = total;
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(135000.0));
}

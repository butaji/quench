//! Shared OSR: a hot loop enters its compiled region mid-run, and the run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use super::shared_osr2::main_osr;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use std::cell::RefCell;
use crate::test_oracle::{assert_tier_agrees, Expect, Tier};

#[test]
fn default_osr_integer_bound_admission_preserves_fractional_and_effectful_comparisons() {
    let source = r#"
        function pipe(){
            const limit=globalThis.limit;
            function step(x){for(let k=0;k<1;k=(k+1)|0)x=(x+1)|0;return x;}
            let sum=0;
            for(let i=0;i<limit;i=(i+1)|0)sum=step(sum);
            return sum;
        }
        let comparisons=0;
        for(const limit of [-1,0,1,0.5,1.5,2000.5,NaN,'2000.5',2000n,
            {valueOf(){comparisons++;return 2000.5;}}]){
            globalThis.limit=limit;
            print(pipe());
        }
        print(comparisons);
    "#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    assert_eq!(run.outcome.printed, ["0", "0", "1", "1", "2", "2001", "0", "2001", "2000", "2001", "2002"]);
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
    // (`PolicyTiers::loop_tier`); the result is the same either way.
    for policy in quench_runtime::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), ["4999500000"], "{policy:?}");
        let osr = main_osr(&compiled, function_id);
        // Loop sites are analyzed only where the JavaScript tiers run (`PolicyTiers::js_function_tiers`).
        let analyzes = quench_runtime::vm::tier_policy::tier_may_run(policy, quench_runtime::jit_mode::Tier::ANY);
        assert_eq!(osr.admitted_sites, usize::from(analyzes), "field loop admission under {policy:?}");
        let compiles = crate::compiled::loop_tier_compiles(policy);
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
    for policy in quench_runtime::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), ["1201100"], "{policy:?}");
        let osr = main_osr(&compiled, function_id);
        // Loop sites are analyzed only where the JavaScript tiers run (`PolicyTiers::js_function_tiers`).
        let analyzes = quench_runtime::vm::tier_policy::tier_may_run(policy, quench_runtime::jit_mode::Tier::ANY);
        assert_eq!(osr.admitted_sites, usize::from(analyzes), "field-reading store loop admission under {policy:?}");
        let compiles = crate::compiled::loop_tier_compiles(policy);
        assert_eq!(osr.currently_compiled_sites, usize::from(compiles), "store loop native code under {policy:?}");
    }
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
    for policy in quench_runtime::MemoryPolicy::ALL {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

        assert_eq!(output.borrow().as_slice(), ["6003994 2000 2000 | 6003934 2000 2000 22"], "{policy:?}");
        if !crate::compiled::loop_tier_compiles(policy) {
            continue;
        }
        let osr = main_osr(&compiled, function_id);
        assert!(osr.native_region_runs > 0, "no native run under {policy:?}: {osr:?}");
        // A store leaves only where the backing is full, once per capacity growth.
        assert!(osr.exits < osr.native_region_runs, "appends left native code under {policy:?}: {osr:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_wavefront_pairs_keep_stencil_sweep_results() {
    // A Gauss-Seidel sweep's row loop runs natively: each case reads what program order
    // reads, bit for bit: an odd last row, integral results,
    // signed zeros and NaN, an element whose `valueOf` runs (which restores the pair and
    // leaves for the general path), and an array read and written as one (not entered).
    let source = r#"
      function bump(x) { x[0] = x[0] + 1; }
      function sweep(x, x0, a, c, width, height, rowSize, sweeps) {
        var invC = 1 / c;
        for (var k = 0; k < sweeps; k++) {
          for (var j = 1; j <= height; j++) {
            var lastRow = (j - 1) * rowSize, currentRow = j * rowSize, nextRow = (j + 1) * rowSize;
            var lastX = x[currentRow];
            ++currentRow;
            for (var i = 1; i <= width; i++)
              lastX = x[currentRow] = (x0[currentRow] + a * (lastX + x[++currentRow] + x[++lastRow] + x[++nextRow])) * invC;
          }
          bump(x);
        }
      }
      function make(n, f) { var a = []; for (var i = 0; i < n; i++) a.push(f(i)); return a; }
      function digest(x) { var h = 0; for (var i = 0; i < x.length; i++) h = (h * 31 + (Object.is(x[i], -0) ? 7 : x[i] !== x[i] ? 11 : Math.floor(x[i] * 4096))) | 0; return h; }
      var log = [], out = [];
      function run(width, height, a, c, fill, fill0, alias, poke) {
        var rowSize = width + 2, n = rowSize * (height + 2);
        var x = make(n, fill), x0 = alias ? x : make(n, fill0);
        if (poke) x[3 * rowSize + 4] = { valueOf: function () { log.push("valueOf"); return 2; } };
        for (var r = 0; r < 40; r++) sweep(x, x0, a, c, width, height, rowSize, 3);
        out.push(digest(x));
      }
      run(8, 6, 1, 4, function (i) { return (i % 7) * 0.25 + 0.5; }, function (i) { return (i % 5) * 0.125; }, false, false);
      run(8, 7, 1, 4, function (i) { return (i % 7) * 0.25 + 0.5; }, function (i) { return (i % 5) * 0.125; }, false, false);
      run(6, 4, 0, 1, function (i) { return i % 3; }, function (i) { return i % 9 - 4; }, false, false);
      run(6, 4, 1, 3, function (i) { return i === 20 ? NaN : -0; }, function (i) { return i % 4 ? -0 : 0; }, false, false);
      run(8, 6, 1, 4, function (i) { return (i % 7) * 0.25 + 0.5; }, function (i) { return (i % 5) * 0.125; }, false, true);
      run(8, 6, 0.5, 2.5, function (i) { return (i % 7) * 0.25 + 0.5; }, null, true, false);
      print(out.join(" ") + " " + log.length);
    "#;
    let expected = ["2081041934 -575006534 -311107584 -869203481 2081041934 -1370797326 2"];
    let program = Engine::specialize(source, "osr-wavefront.js").unwrap();
    let interpreted = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted.clone())), &program).unwrap();
    assert_eq!(interpreted.borrow().as_slice(), expected);
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "sweep"))
        .unwrap();
    for policy in quench_runtime::MemoryPolicy::ALL {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();
        assert_eq!(output.borrow().as_slice(), expected, "{policy:?}");
        if !crate::compiled::loop_tier_compiles(policy) {
            continue;
        }
        let osr = main_osr(&compiled, function_id);
        assert!(osr.native_region_runs > 0, "no native sweep under {policy:?}: {osr:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_stores_fields_of_receivers_the_loop_redefines() {
    // A loop walks a list, storing a field of each node it reaches and reading the same name
    // through the node it moved to (V8-v7 Splay's `splay_` rotations): each store goes through
    // the cached store at its instruction and each read sees the current slot; a node of
    // another shape leaves at the store with nothing written.
    let source = r#"
      function Node(v) { this.v = v; this.next = null; this.seen = 0; }
      function walk(head) {
        var node = head, total = 0;
        while (node) {
          node.seen = node.v * 2 + 1;
          total = total + node.seen;
          node = node.next;
        }
        return total;
      }
      var head = null;
      for (var i = 0; i < 64; i++) { var n = new Node(i); n.next = head; head = n; }
      var odd = { v: 5, next: null, other: 1, seen: 0 };
      var sum = 0;
      for (var r = 0; r < 400; r++) {
        if (r === 300) { odd.next = head; head = odd; }
        sum = (sum + walk(head)) | 0;
      }
      print(sum + " " + head.seen + " " + odd.seen);
    "#;
    let program = Engine::specialize(source, "osr-dynamic-store.js").unwrap();
    let interpreted = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted.clone())), &program).unwrap();
    let expected = interpreted.borrow().clone();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "walk"))
        .unwrap();
    for policy in quench_runtime::MemoryPolicy::ALL {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();
        assert_eq!(*output.borrow(), expected, "{policy:?}");
        if !crate::compiled::loop_tier_compiles(policy) {
            continue;
        }
        let osr = main_osr(&compiled, function_id);
        assert!(osr.native_region_runs > 0, "no native walk under {policy:?}: {osr:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_appends_past_a_full_backing_keep_the_region_native() {
    // Each call appends to a fresh array past its backing's capacity (V8-v7 Crypto's
    // `bnpMultiplyTo` into a new `BigInteger`): the general path grows the backing and the
    // region keeps running natively, through more such exits than a region may take of
    // values leaving the subset.
    let source = r#"
      function fill(n) {
        var a = [];
        for (var i = 0; i < n; i++) a[i] = (i * 7 + 3) & 255;
        return a;
      }
      var sum = 0;
      for (var k = 0; k < 3000; k++) {
        var a = fill(40 + (k & 7));
        sum = (sum * 31 + a[a.length - 1] + a.length) | 0;
      }
      print(sum);
    "#;
    let program = Engine::specialize(source, "osr-append-grow.js").unwrap();
    let interpreted = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted.clone())), &program).unwrap();
    let expected = interpreted.borrow().clone();
    let function_id = program.functions.iter().position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "fill")).unwrap();
    for policy in quench_runtime::MemoryPolicy::ALL {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();
        assert_eq!(*output.borrow(), expected, "{policy:?}");
        if !crate::compiled::loop_tier_compiles(policy) {
            continue;
        }
        let osr = main_osr(&compiled, function_id);
        // The lane path enters the loop once per call and leaves only where a reserve or
        // collection debt needs the VM; the old lowering re-entered at each capacity growth.
        assert!(osr.native_region_runs > 0, "fill left native code under {policy:?}: {osr:?}");
        assert!(osr.exits < osr.native_region_runs, "{policy:?}: {osr:?}");
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_returns_inside_a_loop_resume_the_lane_at_the_return() {
    // A loop that returns from inside its body runs natively and leaves at the `Return`,
    // which the lane runs with the state written back; leaving so never retires the loop.
    let source = r#"
      function cmp(a, b) { var i = a.length; while (--i >= 0) { var r = a[i] - b[i]; if (r != 0) return r; } return 0; }
      var a = [], b = [];
      for (var i = 0; i < 200; i++) { a.push(i * 3 & 255); b.push(i * 3 & 255); }
      var s = 0;
      for (var k = 0; k < 3000; k++) {
        b[k % 200] = (k & 1) ? a[k % 200] + 1 : a[k % 200];
        s = (s + cmp(a, b) * (k % 7)) | 0;
        b[k % 200] = a[k % 200];
      }
      print(s);
    "#;
    let program = Engine::specialize(source, "osr-loop-return.js").unwrap();
    let interpreted = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted.clone())), &program).unwrap();
    assert_eq!(interpreted.borrow().as_slice(), ["-4498"]);
    let function_id = program.functions.iter().position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "cmp")).unwrap();
    for policy in quench_runtime::MemoryPolicy::ALL {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        compiled.programs.enable_js_jit_diagnostics();
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["-4498"], "{policy:?}");
        if !crate::compiled::loop_tier_compiles(policy) {
            continue;
        }
        let views = compiled.programs.views_in_lane(quench_runtime::vm::program_store::ProgramId::MAIN).expect("main program lane views");
        let osr = views
            .get(function_id)
            .and_then(quench_runtime::vm::program_store::derived_lane_view)
            .expect("hot cmp lane view")
            .osr_diagnostics();
        // The lane path runs the loop natively once per call and completes at the Return;
        // the old lowering re-entered at each loop continuation.
        assert!(osr.native_region_runs > 0, "{policy:?}: {osr:?}");
        assert_eq!(osr.exits, 0, "{policy:?}: {osr:?}");
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
    for policy in quench_runtime::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), ["804092460 34 110"], "{policy:?}");
        let osr = main_osr(&compiled, function_id);
        // Loop sites are analyzed only where the JavaScript tiers run (`PolicyTiers::js_function_tiers`).
        let analyzes = quench_runtime::vm::tier_policy::tier_may_run(policy, quench_runtime::jit_mode::Tier::ANY);
        assert_eq!(osr.admitted_sites, usize::from(analyzes), "field-array loop admission under {policy:?}");
        let compiles = crate::compiled::loop_tier_compiles(policy);
        assert_eq!(osr.currently_compiled_sites, usize::from(compiles), "field-array loop native code under {policy:?}");
    }
}

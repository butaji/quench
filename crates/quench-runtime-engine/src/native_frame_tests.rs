//! Native frames in the lane stack's activations (`dispatch_fast::native_frames`): the tag and
//! top word emitted code writes through, and chains of direct calls whose helpers, collections,
//! exceptions and leaves must match the lane's own run.
use quench_runtime::vm::engine_types::DirectBody;
use crate::dispatch_fast::{KIND_NATIVE, KIND_OFFSET};
use quench_runtime::staged::frame;
use quench_runtime::vm::lane_activations::LaneActivations;
use crate::engine_prelude::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use std::{cell::RefCell, rc::Rc};

/// The output of `source` run with the lane's native bodies on (`native`) or off.
fn run(source: &str, native: bool) -> Vec<String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(quench_runtime::MemoryPolicy::Throughput);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    if native { vm.programs.enable_js_lane_methods(); }
    let program = Engine::specialize(source, "native-frames.js").unwrap();
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    output.borrow().clone()
}

fn same_with_and_without_native_bodies(source: &str) {
    let expected = run(source, false);
    assert!(!expected.is_empty(), "the fixture prints");
    assert_eq!(run(source, true), expected);
}

/// A chain of `depth` functions, each calling the next with its own argument, ending in
/// `innermost`: every call is one a native body makes directly once its callee has a body.
fn chain(depth: usize, innermost: &str, wrap: impl Fn(usize) -> String) -> String {
    let mut source = format!("function c{depth}(n) {{ {innermost} }}\n");
    for level in (0..depth).rev() {
        source += &format!("function c{level}(n) {{ {} }}\n", wrap(level).replace("NEXT", &format!("c{}(n + {level})", level + 1)));
    }
    source
}

#[test]
fn native_frames_pushed_through_the_top_word_are_counted_until_linked() {
    let activations = LaneActivations::with_capacity(4);
    // A frame that still runs its body: `top_native` names only those (`retire_bodies` clears it).
    let callee = std::ptr::dangling::<DirectBody>();
    assert!(!activations.top_is_native());
    for pushed in 1..=2 {
        // SAFETY: the allocation holds four slots; emitted code writes the prefix, the tag and the top the same way.
        unsafe {
            let top = *activations.top_word();
            top.cast::<u8>().add(frame::CALLEE).cast::<*const DirectBody>().write(callee);
            top.cast::<u8>().add(frame::BODY).cast::<usize>().write(1);
            top.cast::<u8>().add(KIND_OFFSET).write(KIND_NATIVE);
            *activations.top_word() = top.add(1);
        }
        assert!(activations.top_is_native());
        assert_eq!((activations.len(), activations.unlinked(), activations.native_depth()), (pushed, pushed, pushed));
    }
    assert!(activations.last().is_some_and(|top| top.callee == callee && top.body == 1));
}

#[test]
fn deep_native_call_chains_run_a_helper_at_the_innermost_level() {
    let mut source = chain(40, "var p = new Point(n, 2); return p.x + p.y + String(n).length + [n, 1].length;", |_| "return NEXT + 1;".into());
    source += r#"
      function Point(x, y) { this.x = x; this.y = y; }
      var total = 0;
      for (var i = 0; i < 3000; i++) total = (total + c0(i & 255)) | 0;
      print(total);
    "#;
    same_with_and_without_native_bodies(&source);
}

#[test]
fn collections_run_under_native_call_chains() {
    let mut source = chain(12, "return { v: [n, n + 1, n + 2] };", |level| format!("var keep = [{level}, n, {{ k: n }}]; var r = NEXT; return {{ v: r.v.concat(keep[2].k, keep.length) }};"));
    source += r#"
      var sum = 0;
      for (var i = 0; i < 6000; i++) { var r = c0(i & 63); sum = (sum + r.v.length + r.v[3]) | 0; }
      print(sum);
    "#;
    same_with_and_without_native_bodies(&source);
}

#[test]
fn exceptions_cross_native_frames_to_the_handler_at_any_depth() {
    let mut source = chain(10, "if (n % 7 === 0) throw new Error('inner' + n); return n;", |level| {
        if level % 3 == 1 { format!("try {{ return NEXT + 1; }} catch (e) {{ return e.message.length - {level}; }}") } else { "return NEXT + 1;".into() }
    });
    source += r#"
      var total = 0, caught = 0;
      for (var i = 0; i < 4000; i++) {
        try { total = (total + c0(i & 63)) | 0; } catch (e) { caught = (caught + e.message.length) | 0; }
      }
      print(total + " " + caught);
    "#;
    same_with_and_without_native_bodies(&source);
}

#[test]
fn a_leave_at_each_depth_of_a_native_chain_materializes_its_frames() {
    const DEPTH: usize = 8;
    // Level `L` sees `n + L (L - 1) / 2` of the argument, so `n` for `at` makes level `at` leave
    // the lane (guest code from a native), whatever the levels above it do.
    let mut source = chain(DEPTH, "return n;", |level| format!(
        "if (n === {}) return JSON.stringify({{ toJSON() {{ return n + {level}; }} }}).length + [n].map(x => x + {level})[0]; return NEXT + 1;", 1000 + level));
    source += &format!(r#"
      var total = 0;
      for (var i = 0; i < 3000; i++) {{
        var at = i % {DEPTH};
        total = (total + c0(1000 + at - at * (at - 1) / 2)) | 0;
      }}
      print(total);
    "#);
    same_with_and_without_native_bodies(&source);
}

#[test]
fn a_native_callee_calls_through_the_helper_path_above_its_frames() {
    // The innermost level calls one of eight closures, more than a site inlines or calls
    // directly: its activation is the helper's, pushed above the chain's native frames.
    let mut source = chain(6, "return handlers[n & 7](n);", |_| "return NEXT + 1;".into());
    source += r#"
      function make(k) { return function (n) { return n * k + 1; }; }
      var handlers = [];
      for (var k = 0; k < 8; k++) handlers.push(make(k));
      var total = 0;
      for (var i = 0; i < 4000; i++) total = (total + c0(i & 255)) | 0;
      print(total);
    "#;
    same_with_and_without_native_bodies(&source);
}

#[test]
fn recursion_past_the_native_frame_bound_continues_in_helper_activations() {
    let source = r#"
      function down(n) { return n === 0 ? 0 : 1 + down(n - 1); }
      var total = 0;
      for (var i = 0; i < 600; i++) total = (total + down(150 + (i & 15))) | 0;
      print(total);
    "#;
    same_with_and_without_native_bodies(source);
}

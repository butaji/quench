//! The name, environment-local, capture and `with` steps of a compiled callee that runs as a
//! native callee's activation (`dispatch_fast::vm_steps::owner_step_in`, `Vm::running_owner`)
//! are made over the activation's window: a chain of compiled callees whose innermost ones make
//! them leaves none of its runs, and prints what the interpreter prints.
use crate::vm::catch_tests::{bodies_match_interpreter, BodyRuns};
use crate::vm::test_oracle::Tier;

/// Ten chained functions: `c9` reads and writes names (a `typeof` of an unbound name too), `c8` reads
/// and writes a local its closure captures, `c7` looks its names up through a `with` object, and each
/// other level calls the next.
const CHAIN: &str = r#"
    var G = 3;
    function c9(n) {
      G = (G + 1) & 255;
      var seen = typeof unbound === "undefined" ? 1 : 0;
      return G + n + seen;
    }
    function c8(n) {
      var hits = n;
      var read = function () { return hits; };
      hits = hits + read();
      return hits + c9(n);
    }
    function c7(n) {
      var scope = { a: n, b: 2 };
      with (scope) { a = a + b; return a + c8(b); }
    }
    function c6(n) { return c7(n + 1) + 1; }
    function c5(n) { return c6(n + 1) + 1; }
    function c4(n) { return c5(n + 1) + 1; }
    function c3(n) { return c4(n + 1) + 1; }
    function c2(n) { return c3(n + 1) + 1; }
    function c1(n) { return c2(n + 1) + 1; }
    function c0(n) { return c1(n + 1) + 1; }
    var total = 0;
    for (var i = 0; i < 6000; i++) total = (total + c0(i & 31)) | 0;
    print(total + "," + G);
"#;

const NAMES: [&str; 10] = ["c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9"];

/// The body runs of the chain under `tier`, whose outcome equals the interpreter's (`bodies_match_interpreter`).
fn chain_runs(tier: Tier) -> Vec<BodyRuns> {
    bodies_match_interpreter(CHAIN, tier, &NAMES)
}

/// Every body ran natively to its end (`ended`) and none left for the general path.
fn all_native_and_none_left(runs: &[BodyRuns]) {
    for (name, (ended, left)) in NAMES.iter().zip(runs) {
        assert!(*ended > 0, "{name} was never entered natively");
        assert_eq!(*left, 0, "a run of {name} left for the general path");
    }
}

#[test]
fn a_chain_of_name_local_and_with_steps_runs_natively_without_leaving() {
    all_native_and_none_left(&chain_runs(Tier::Full));
}

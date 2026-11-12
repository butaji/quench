//! The forms a whole-function body makes in place: each has its row (or its helper step), and a body
//! that holds it runs as the interpreter does, with the interpreter's step behind a fast arm.
use super::tests::run_both;
use crate::bytecode::Op;
use crate::vm::compiled::base_region::fast_rows;

/// A read of a binding in its temporal dead zone, and an assignment that is no initialization, leave a fast row
/// into the interpreter's step, which raises; an initialized binding is read and written by the row.
#[test]
fn local_reads_and_assignments_check_their_temporal_dead_zone_in_a_row() {
    let ops = [Op::LoadLocal, Op::StoreLocal];
    let before = ops.map(fast_rows);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function reads(n) { let s = 0; for (let i = 0; i < n; i++) { let a = i; s = s + a; { let b = s; b = b + 1; s = b; } } return s; }
         function dead(n) { let s = 0; for (let i = 0; i < n; i++) { try { s = s + z; } catch (e) { s = s + 1; } try { z = 2; } catch (e) { s = s + 2; } let z = 1; } return s; }
         let t = 0; for (let m = 0; m < 300; m++) t = (t + reads(m % 6) + dead(m % 4)) | 0;
         print(t); print(reads(4)); print(dead(3));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    for (op, before) in ops.into_iter().zip(before) {
        assert!(fast_rows(op) > before, "{op:?} has no fast row");
    }
}

/// `typeof v` as a string is the entry of the VM's table of result strings its tag selects, so the strings are
/// the ones every `typeof` and every string constant spelling one yield (identity under `===`), for every kind of
/// value; a heap value leaves the row for the interpreter's step.
#[test]
fn typeof_strings_are_the_interned_table_entries() {
    let before = fast_rows(Op::Unary);
    let (interpreted, compiled, [runs, _]) = run_both(
        "function name(v) { return typeof v; }
         function same(v) { const s = typeof v; return s === 'number' || s === 'undefined' || s === 'object'; }
         function ident(v) { return typeof v === typeof name; }
         function counted(n) { let c = ''; for (let i = 0; i < n; i++) c = typeof i; return c; }
         const vs = [1, 2.5, 'a', undefined, null, true, {}, function () {}, Symbol(), 1n]; let t = 0, s = '';
         for (let k = 0; k < 300; k++) { const v = vs[k % 10]; s = name(v); t = (t + s.length + (same(v) ? 1 : 0) + (ident(v) ? 2 : 0) + counted(k % 3).length) | 0; }
         print(t); print(s); print(name(3)); print(name(null)); print(name(() => 1)); print(counted(2));",
    );
    assert_eq!(interpreted, compiled);
    assert!(runs > 100, "the bodies ran {runs} times");
    assert!(fast_rows(Op::Unary) > before, "no typeof string fast row");
}
